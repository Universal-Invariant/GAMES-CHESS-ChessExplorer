//! Chess engine connectivity (modular):
//!  - `UciEngine`: spawns any UCI engine (Stockfish, lc0, nibbler-wrapped nets)
//!    with configurable binary/args; blocking go/movetime analysis.
//!  - `BuiltinEngine`: small alpha-beta + PST engine so the app is fully
//!    functional even without an external binary installed.
//! Both implement `ChessEngine` — analysis modules can be pointed at either.

use crate::analysis::pst_eval;
use crate::board::{piece_type, State, Move};
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

#[derive(Clone, Debug, Serialize)]
pub struct EngineEval {
    pub cp: i32, // centipawns, white's perspective (mate scores clamped big)
    pub mate: Option<i32>,
    pub best: String, // uci move
    pub pv: Vec<String>,
    pub depth: u32,
}

pub trait ChessEngine {
    /// Evaluate position after best reply search within `ms` budget.
    fn evaluate(&mut self, fen: &str, ms: u64) -> Result<EngineEval, String>;
    fn name(&self) -> &str;
}

// ---------------------------------------------------------------------------
// UCI wrapper
// ---------------------------------------------------------------------------

pub struct UciEngine {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    pub name: String,
}

impl UciEngine {
    /// binary e.g. "stockfish" or "/usr/games/lc0", args e.g. ["-d","12"] for lc0 backends.
    pub fn spawn(binary: &str, args: &[&str]) -> Result<UciEngine, String> {
        let mut child = Command::new(binary)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn {}: {}", binary, e))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("no stdout")?);
        let mut eng = UciEngine {
            child,
            stdin,
            stdout,
            name: binary.to_string(),
        };
        eng.send("uci");
        eng.read_until("uciok")?;
        eng.send("isready");
        eng.read_until("readyok")?;
        Ok(eng)
    }

    fn send(&mut self, cmd: &str) {
        let _ = writeln!(self.stdin, "{}", cmd);
        let _ = self.stdin.flush();
    }

    fn read_line(&mut self) -> Result<String, String> {
        let mut line = String::new();
        let n = self
            .stdout
            .read_line(&mut line)
            .map_err(|e| format!("engine read error: {}", e))?;
        if n == 0 {
            return Err("engine closed".into());
        }
        Ok(line.trim_end().to_string())
    }

    fn read_until(&mut self, token: &str) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        for _ in 0..10000 {
            let l = self.read_line()?;
            let done = l == token;
            lines.push(l);
            if done {
                return Ok(lines);
            }
        }
        Err(format!("timeout waiting for {}", token))
    }

    pub fn set_option(&mut self, name: &str, value: &str) {
        self.send(&format!("setoption name {} value {}", name, value));
    }
}

impl ChessEngine for UciEngine {
    fn evaluate(&mut self, fen: &str, ms: u64) -> Result<EngineEval, String> {
        self.send("ucinewgame");
        self.send(&format!("position fen {}", fen));
        self.send(&format!("go movetime {}", ms.max(50)));
        let lines = self.read_until("bestmove")?;
        parse_ucibest(&lines).ok_or_else(|| "could not parse engine output".to_string())
    }
    fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for UciEngine {
    fn drop(&mut self) {
        self.send("quit");
        let _ = self.child.kill();
    }
}

fn parse_ucibest(lines: &[String]) -> Option<EngineEval> {
    let best_line = lines.iter().rev().find(|l| l.starts_with("bestmove"))?;
    let best = best_line.split_whitespace().nth(1)?.to_string();
    // last info line with score
    let info = lines.iter().rev().find(|l| l.starts_with("info") && (l.contains("score")))?.clone();
    let toks: Vec<&str> = info.split_whitespace().collect();
    let mut cp = 0i32;
    let mut mate: Option<i32> = None;
    let mut depth = 0u32;
    let mut pv: Vec<String> = Vec::new();
    let mut i = 0;
    let mut after_pv = false;
    while i < toks.len() {
        match toks[i] {
            "depth" => {
                if i + 1 < toks.len() {
                    depth = toks[i + 1].parse().unwrap_or(0);
                }
                i += 1;
            }
            "score" => {
                if i + 2 < toks.len() {
                    match toks[i + 1] {
                        "cp" => cp = toks[i + 2].parse().unwrap_or(0),
                        "mate" => mate = toks[i + 2].parse().ok(),
                        _ => {}
                    }
                }
                i += 2;
            }
            "pv" => after_pv = true,
            _ => {
                if after_pv {
                    pv.push(toks[i].to_string());
                }
            }
        }
        i += 1;
    }
    if let Some(m) = mate {
        cp = if m > 0 { 99999 - m * 100 } else { -99999 + m * 100 };
    }
    Some(EngineEval {
        cp,
        mate,
        best,
        pv,
        depth,
    })
}

// ---------------------------------------------------------------------------
// Built-in engine: iterative-deepening alpha-beta with material+PST eval
// ---------------------------------------------------------------------------

pub struct BuiltinEngine {
    pub max_depth: u32,
}

impl Default for BuiltinEngine {
    fn default() -> Self {
        Self { max_depth: 4 }
    }
}

const MAT: [i32; 7] = [0, 100, 320, 330, 500, 900, 0];

fn eval_side(st: &State) -> i32 {
    // positive = good for side-to-move
    pst_eval(st)
}

fn order_moves(st: &State, mv: &mut [Move]) {
    mv.sort_by_key(|m| {
        let cap = st.board[m.to as usize];
        -(MAT[piece_type(cap) as usize] * 8 + if m.promo != 0 { 700 } else { 0 })
    });
}

fn alphabeta(st: &State, depth: u32, mut alpha: i32, beta: i32, ply: u32) -> i32 {
    if depth == 0 {
        // quiescence-lite: only resolve captures up to 2 extra plies
        let stand = eval_side(st);
        if ply > 6 {
            return stand;
        }
        let mut best = stand;
        let caps: Vec<Move> = st
            .generate_moves()
            .into_iter()
            .filter(|m| st.board[m.to as usize] != 0 || m.flag == crate::board::F_EP)
            .collect();
        for m in caps {
            let ns = st.make(m);
            if ns.in_check(st.side) {
                continue;
            }
            let v = -alphabeta(&ns, 0, -beta, -alpha, ply + 1);
            if v > best {
                best = v;
                if best >= beta {
                    return best;
                }
            }
        }
        return best;
    }
    let mut moves = st.legal_moves();
    if moves.is_empty() {
        return if st.in_check(st.side) {
            -100000 + ply as i32
        } else {
            0
        };
    }
    order_moves(st, &mut moves);
    let mut best = -1000000;
    for m in moves {
        let ns = st.make(m);
        let v = -alphabeta(&ns, depth - 1, -beta, -alpha, ply + 1);
        if v > best {
            best = v;
            if v > alpha {
                alpha = v;
                if alpha >= beta {
                    break;
                }
            }
        }
    }
    best
}

impl ChessEngine for BuiltinEngine {
    fn evaluate(&mut self, fen: &str, ms: u64) -> Result<EngineEval, String> {
        let start = std::time::Instant::now();
        let st = State::from_fen(fen).ok_or("bad fen")?;
        let mut best_now = EngineEval {
            cp: eval_side(&st),
            mate: None,
            best: String::new(),
            pv: Vec::new(),
            depth: 0,
        };
        let mut moves = st.legal_moves();
        if moves.is_empty() {
            return Ok(best_now);
        }
        for d in 1..=self.max_depth {
            order_moves(&st, &mut moves);
            let mut local_best: Option<(i32, Move)> = None;
            let mut alpha = -1000000i32;
            for m in &moves {
                let ns = st.make(*m);
                let v = -alphabeta(&ns, d - 1, -1000000, -alpha, 1);
                if local_best.map(|(bv, _)| v > bv).unwrap_or(true) {
                    local_best = Some((v, *m));
                    if v > alpha {
                        alpha = v;
                    }
                }
                if start.elapsed().as_millis() as u64 > ms {
                    break;
                }
            }
            if let Some((v, m)) = local_best {
                best_now = EngineEval {
                    cp: v,
                    mate: None,
                    best: crate::pgn::uci_string(m),
                    pv: vec![crate::pgn::uci_string(m)],
                    depth: d,
                };
            }
            if start.elapsed().as_millis() as u64 > ms {
                break;
            }
        }
        // convert from side-to-move perspective to white perspective
        let cp = if st.side == 0 { best_now.cp } else { -best_now.cp };
        best_now.cp = cp.clamp(-99999, 99999);
        Ok(best_now)
    }
    fn name(&self) -> &str {
        "builtin-minimax"
    }
}

/// Try spawning a real engine, fall back to builtin. Returns (engine, kind).
pub fn get_engine(preferred: Option<&str>) -> Box<dyn ChessEngine> {
    if let Some(bin) = preferred {
        if let Ok(e) = UciEngine::spawn(bin, &[]) {
            return Box::new(e);
        }
        // try common locations
        for cand in ["stockfish", "/usr/games/stockfish", "/usr/local/bin/stockfish", "lc0"] {
            if let Ok(e) = UciEngine::spawn(cand, &[]) {
                return Box::new(e);
            }
        }
    } else {
        for cand in ["stockfish", "/usr/games/stockfish", "/usr/local/bin/stockfish"] {
            if let Ok(e) = UciEngine::spawn(cand, &[]) {
                return Box::new(e);
            }
        }
    }
    Box::new(BuiltinEngine::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_finds_free_piece_and_mate() {
        let mut e = BuiltinEngine { max_depth: 3 };
        // white rook can capture the undefended black queen on d8
        let ev = e.evaluate("3qk3/8/8/8/8/8/8/R3K3 w - - 0 1", 300).unwrap();
        assert_eq!(ev.best, "a1d8");
        // mate in 1 for white: back-rank
        let ev2 = e.evaluate("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1", 300).unwrap();
        assert_eq!(ev2.best, "a1a8");
        assert!(ev2.cp > 50000);
    }

    #[test]
    fn builtin_prefers_promo_capture() {
        let mut e = BuiltinEngine { max_depth: 4 };
        let ev = e.evaluate("4k3/P7/8/8/8/8/8/4K3 w - - 0 1", 300).unwrap();
        // a7 promotion should win ~queen value over king shuffles
        assert!(ev.best.starts_with("a7"));
    }
}
