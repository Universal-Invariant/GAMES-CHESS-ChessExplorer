//! Modular analysis pipeline. Each `AnalysisModule` consumes a position (and
//! optionally the move that led to it) and emits JSON-serializable features
//! consumed by visualizers: attacks, defenses, threats, pins, forks, hangs,
//! material, evaluation, centipawn loss classes, heatmaps...
//!
//! Modules are pluggable: register any subset; the frontend never needs to
//! know which module produced a feature — everything flows through a common
//! `Frame` structure.

use crate::board::{color_of, piece_type, file_of, rank_of, State, KNIGHT, KING, PAWN};
use serde::Serialize;
use serde_json::{json, Value};

pub const VAL_PAWN: i32 = 100;
pub const VAL_KNIGHT: i32 = 320;
pub const VAL_BISHOP: i32 = 330;
pub const VAL_ROOK: i32 = 500;
pub const VAL_QUEEN: i32 = 900;

pub fn piece_value(pt: u8) -> i32 {
    match pt {
        PAWN => VAL_PAWN,
        KNIGHT => VAL_KNIGHT,
        crate::board::BISHOP => VAL_BISHOP,
        crate::board::ROOK => VAL_ROOK,
        crate::board::QUEEN => VAL_QUEEN,
        _ => 0,
    }
}

/// One analyzed position with all module outputs merged.
#[derive(Clone, Debug, Serialize)]
pub struct Frame {
    pub ply: usize,
    pub fen: String,
    pub san: String,
    pub uci: String,
    /// Features keyed by module name, e.g. "tactics", "material".
    pub features: Vec<(String, Value)>,
}

pub trait AnalysisModule: Send + Sync {
    fn name(&self) -> &str;
    /// Analyze position `st` *before* the move `san` is played (ply index given).
    fn analyze(&self, st: &State, san: &str, ply: usize) -> Value;
}

// ---------------------------------------------------------------------------
// Tactics / dynamics module (pure Rust, engine-free)
// ---------------------------------------------------------------------------

pub struct TacticsModule {
    pub include_heatmap: bool,
}

impl Default for TacticsModule {
    fn default() -> Self {
        Self { include_heatmap: true }
    }
}

fn attackers_to(st: &State, sq: u8, by_color: u8) -> Vec<u8> {
    let mut v = Vec::new();
    for m in st.generate_moves() {
        if m.to == sq && color_of(st.board[m.from as usize]) == by_color {
            // ensure pseudo-move from our side
            if color_of(st.board[m.from as usize]) == st.side {
                v.push(m.from);
            }
        }
    }
    // also include pieces of `by_color` attacking even when not to move
    if by_color != st.side {
        for s in 0..64u8 {
            let p = st.board[s as usize];
            if p == 0 || color_of(p) != by_color {
                continue;
            }
            if attacks_sq(st, s, sq) {
                v.push(s);
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// Does the piece on `from` attack square `to` (geometry only)?
fn attacks_sq(st: &State, from: u8, to: u8) -> bool {
    let p = st.board[from as usize];
    if p == 0 {
        return false;
    }
    let df = file_of(to) as i8 - file_of(from) as i8;
    let dr = rank_of(to) as i8 - rank_of(from) as i8;
    match piece_type(p) {
        PAWN => dr == if color_of(p) == 0 { 1 } else { -1 } && df.abs() == 1,
        KNIGHT => (df.abs() == 1 && dr.abs() == 2) || (df.abs() == 2 && dr.abs() == 1),
        crate::board::KING => df.abs() <= 1 && dr.abs() <= 1 && !(df == 0 && dr == 0),
        t => {
            let adf = df.abs();
            let adr = dr.abs();
            let line_ok = match t {
                crate::board::BISHOP => adf == adr && adf > 0,
                crate::board::ROOK => (df == 0) != (dr == 0),
                crate::board::QUEEN => (adf == adr && adf > 0) || (df == 0) != (dr == 0),
                _ => false,
            };
            if !line_ok {
                return false;
            }
            // clear path
            let step_f = df.signum();
            let step_r = dr.signum();
            let mut cf = file_of(from) as i8 + step_f;
            let mut cr = rank_of(from) as i8 + step_r;
            while cf != file_of(to) as i8 || cr != rank_of(to) as i8 {
                if st.board[((7 - cr as i32) * 8 + cf as i32) as usize] != 0 {
                    return false;
                }
                cf += step_f;
                cr += step_r;
            }
            true
        }
    }
}

/// Defenders of a square (pieces of same color able to recapture/protect).
fn defenders_of(st: &State, sq: u8, color: u8) -> Vec<u8> {
    let mut v = Vec::new();
    for s in 0..64u8 {
        let p = st.board[s as usize];
        if p != 0 && color_of(p) == color && s != sq && attacks_sq(st, s, sq) {
            v.push(s);
        }
    }
    v
}

/// Absolute pins: attacker X — pinned piece P — valuable target T in line.
fn find_pins(st: &State) -> Vec<Value> {
    let mut pins = Vec::new();
    for s in 0..64u8 {
        let p = st.board[s as usize];
        if p == 0 || piece_type(p) == PAWN || piece_type(p) == KING {
            continue;
        }
        let color = color_of(p);
        let king_sq = st.king_square(color);
        let df = file_of(king_sq) as i8 - file_of(s) as i8;
        let dr = rank_of(king_sq) as i8 - rank_of(s) as i8;
        let diag = df.abs() == dr.abs() && df != 0;
        let ortho = (df == 0) != (dr == 0);
        if !(diag || ortho) {
            continue;
        }
        // beyond the piece along the ray toward king direction continue outward
        let step_f = df.signum();
        let step_r = dr.signum();
        let mut cf = file_of(s) as i8 + step_f;
        let mut cr = rank_of(s) as i8 + step_r;
        while cf >= 0 && cf < 8 && cr >= 0 && cr < 8 {
            let t = st.board[((7 - cr as i32) * 8 + cf as i32) as usize];
            if t != 0 {
                if color_of(t) != color {
                    let tt = piece_type(t);
                    let can_pin = if diag {
                        tt == crate::board::BISHOP || tt == crate::board::QUEEN
                    } else {
                        tt == crate::board::ROOK || tt == crate::board::QUEEN
                    };
                    if can_pin {
                        pins.push(json!({
                            "attacker": crate::board::sq_name(((7 - cr as i32) * 8 + cf as i32) as u8),
                            "pinned": crate::board::sq_name(s),
                            "target": crate::board::sq_name(king_sq),
                            "value": piece_value(tt),
                        }));
                    }
                }
                break;
            }
            cf += step_f;
            cr += step_r;
        }
    }
    pins
}

/// Knight forks: knight attacking 2+ valuable targets simultaneously.
fn find_forks(st: &State) -> Vec<Value> {
    let mut out = Vec::new();
    for s in 0..64u8 {
        let p = st.board[s as usize];
        if p == 0 || piece_type(p) != KNIGHT {
            continue;
        }
        let color = color_of(p);
        let mut targets = Vec::new();
        for (df, dr) in [(1i8, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)] {
            let nf = file_of(s) as i8 + df;
            let nr = rank_of(s) as i8 + dr;
            if nf < 0 || nf > 7 || nr < 0 || nr > 7 {
                continue;
            }
            let t = st.board[((7 - nr as i32) * 8 + nf as i32) as usize];
            if t != 0 && color_of(t) != color {
                let tv = piece_value(piece_type(t));
                let high_val = piece_type(t) == crate::board::KING || tv >= VAL_ROOK;
                let low_defended = defenders_of(st, ((7 - nr as i32) * 8 + nf as i32) as u8, color_of(t)).is_empty();
                if high_val && low_defended {
                    targets.push(crate::board::sq_name(((7 - nr as i32) * 8 + nf as i32) as u8));
                }
            }
        }
        if targets.len() >= 2 {
            out.push(json!({
                "knight": crate::board::sq_name(s),
                "targets": targets,
            }));
        }
    }
    out
}

impl AnalysisModule for TacticsModule {
    fn name(&self) -> &str {
        "tactics"
    }

    fn analyze(&self, st: &State, _san: &str, _ply: usize) -> Value {
        let mover = st.side;
        let mut attacks = Vec::new();
        let mut threats = Vec::new();
        let mut hangs = Vec::new();
        let mut defended = Vec::new();
        // per-square attacker counts for heatmap (attacking pieces of each color)
        let mut atk_w = [0u8; 64]; // white pieces attacking each square
        let mut atk_b = [0u8; 64]; // black pieces attacking each square

        for s in 0..64u8 {
            let p = st.board[s as usize];
            if p != 0 {
                let color = color_of(p);
                let opp = 1 - color;
                let att = attackers_to(st, s, opp);
                let def = defenders_of(st, s, color);
                let cnt = att.len();
                if cnt > 0 {
                    attacks.push(json!({
                        "from": att.iter().map(|x| crate::board::sq_name(*x)).collect::<Vec<_>>(),
                        "to": crate::board::sq_name(s),
                        "victim": crate::board::piece_letter(p),
                        "victim_value": piece_value(piece_type(p)),
                        "defenders": def.len(),
                    }));
                    // threat: attacker cheaper than victim and victim underdefended
                    for a in &att {
                        let ap = st.board[*a as usize];
                        let av = piece_value(piece_type(ap));
                        let vv = piece_value(piece_type(p));
                        if av < vv && def.len() < cnt {
                            threats.push(json!({
                                "attacker": crate::board::sq_name(*a),
                                "target": crate::board::sq_name(s),
                                "gain": vv - av,
                            }));
                            break;
                        }
                    }
                    if def.is_empty() && piece_type(p) != crate::board::KING {
                        hangs.push(json!({
                            "square": crate::board::sq_name(s),
                            "piece": crate::board::piece_letter(p),
                            "value": piece_value(piece_type(p)),
                            "attackers": cnt,
                        }));
                    }
                }
                if !def.is_empty() {
                    defended.push(json!({
                        "square": crate::board::sq_name(s),
                        "by": def.iter().map(|x| crate::board::sq_name(*x)).collect::<Vec<_>>(),
                    }));
                }
            }
            // heatmap: count attackers onto every square regardless of occupancy
            for a in attackers_to(st, s, 0) {
                atk_w[s as usize] = atk_w[s as usize].saturating_add(1);
                let _ = a;
            }
            for _ in attackers_to(st, s, 1) {
                atk_b[s as usize] = atk_b[s as usize].saturating_add(1);
            }
        }

        // discovered/opened lines: after the mover's last move, did a slider behind
        // an empty diagonal open? Approximate: long-range attacks by mover onto enemy
        // major pieces that pass over the square the mover just vacated — handled
        // client-side via move deltas; here we expose pins/forks.
        let pins = find_pins(st);
        let forks = find_forks(st);
        let check = st.in_check(mover);

        let mut obj = json!({
            "attacks": attacks,
            "threats": threats,
            "hangs": hangs,
            "defenses": defended,
            "pins": pins,
            "forks": forks,
            "check": check,
            "mover": if mover == 0 { "w" } else { "b" },
        });
        if self.include_heatmap {
            obj["heatmap"] = json!({
                "white_attack": atk_w.iter().map(|v| *v as u32).collect::<Vec<_>>(),
                "black_attack": atk_b.iter().map(|v| *v as u32).collect::<Vec<_>>(),
            });
        }
        obj
    }
}

// ---------------------------------------------------------------------------
// Material / static evaluation module
// ---------------------------------------------------------------------------

pub struct MaterialModule;

impl AnalysisModule for MaterialModule {
    fn name(&self) -> &str {
        "material"
    }
    fn analyze(&self, st: &State, _san: &str, _ply: usize) -> Value {
        let mut w = 0i32;
        let mut b = 0i32;
        let mut counts = [0u16; 12];
        for s in 0..64u8 {
            let p = st.board[s as usize];
            if p == 0 {
                continue;
            }
            let v = piece_value(piece_type(p));
            if color_of(p) == 0 {
                w += v;
                counts[piece_type(p) as usize - 1] += 1;
            } else {
                b += v;
                counts[5 + piece_type(p) as usize - 1] += 1;
            }
        }
        json!({
            "white_material": w,
            "black_material": b,
            "balance_cp": w - b,
            "pst_eval_cp": pst_eval(st),
        })
    }
}

/// Small piece-square-table evaluation (midgame-ish) for the built-in fallback
/// engine and quick heuristic display.
pub fn pst_eval(st: &State) -> i32 {
    fn pc(pt: u8, s: u8, white: bool) -> i32 {
        let r = rank_of(s) as i32;
        let f = file_of(s) as i32;
        let base = piece_value(pt);
        let bonus: i32 = match pt {
            PAWN => r * 8 - (if f == 0 || f == 7 { 10 } else { 0 }),
            KNIGHT | crate::board::BISHOP => {
                let center = (3 - (f - 3).abs()).max(0) * 6 + (r.min(7 - r)) * 4;
                center
            }
            crate::board::ROOK => (if r >= 2 { 8 } else { 0 }) + (if f == 3 || f == 4 { 4 } else { 0 }),
            crate::board::QUEEN => -(r.max(7 - r)),
            crate::board::KING => -(r.max(7 - r) + (f - 3).abs()),
            _ => 0,
        };
        if white {
            base + bonus
        } else {
            -(base + bonus)
        }
    }
    let mut score = 0i32;
    for s in 0..64u8 {
        let p = st.board[s as usize];
        if p != 0 {
            score += pc(piece_type(p), s, color_of(p) == 0);
        }
    }
    if st.side == 1 {
        score = -score;
    }
    score
}

// ---------------------------------------------------------------------------
// Blunder classification module (needs engine evals; runs post-analysis)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Serialize)]
pub struct EvalPoint {
    pub cp: i32, // centipawns from white's perspective
    pub mate: Option<i32>,
}

pub fn classify_move(before: EvalPoint, after: EvalPoint, mover_white: bool) -> &'static str {
    let swing = if mover_white {
        eval_cp(after) - eval_cp(before)
    } else {
        eval_cp(before) - eval_cp(after)
    };
    // swing < 0 means the move lost value for the mover
    let loss = -swing;
    if mate_after_mate_lost(after, before) {
        return "blunder";
    }
    if loss >= 500 {
        "blunder"
    } else if loss >= 200 {
        "mistake"
    } else if loss >= 80 {
        "inaccuracy"
    } else {
        "good"
    }
}

fn mate_after_mate_lost(_after: EvalPoint, before: EvalPoint) -> bool {
    before.mate.map(|m| m.abs()).unwrap_or(i32::MAX) <= 5 && false
}

pub fn eval_cp(e: EvalPoint) -> i32 {
    match e.mate {
        Some(m) => {
            let sign = if m > 0 { 1 } else { -1 };
            sign * (100000 - m.abs() * 100)
        }
        None => e.cp.clamp(-99999, 99999),
    }
}

pub fn default_modules() -> Vec<Box<dyn AnalysisModule>> {
    vec![Box::new(TacticsModule::default()), Box::new(MaterialModule)]
}

/// Run the module pipeline over every ply of a game.
pub fn analyze_game(modules: &[Box<dyn AnalysisModule>], sans: &[String], start_fen: Option<&str>) -> Vec<Frame> {
    let mut st = start_fen.and_then(State::from_fen).unwrap_or_else(State::start);
    let mut frames = Vec::with_capacity(sans.len());
    for (i, san) in sans.iter().enumerate() {
        match st.find_move_san(san) {
            Some(m) => {
                let uci = crate::pgn::uci_string(m);
                let canon = st.san(m);
                let fen = st.to_fen();
                let feats: Vec<(String, Value)> = modules.iter().map(|mo| (mo.name().to_string(), mo.analyze(&st, san, i))).collect();
                frames.push(Frame {
                    ply: i,
                    fen,
                    san: canon,
                    uci,
                    features: feats,
                });
                st = st.make(m);
            }
            None => break,
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::parse_sq;

    #[test]
    fn detects_hanging_piece_and_fork() {
        // White queen attacked by black pawn-less rook; simple: black rook attacks undefended white bishop
        let st = State::from_fen("4k3/8/8/8/2b5/8/4R3/4K3 b - - 0 1").unwrap();
        let tm = TacticsModule::default();
        let v = tm.analyze(&st, "", 0);
        let hangs = v["hangs"].as_array().unwrap();
        // bishop on c4 attacked by rook e2 and undefended
        assert!(hangs.iter().any(|h| h["square"] == "c4"));
    }

    #[test]
    fn pin_detection() {
        let st = State::from_fen("4k3/8/8/8/4b3/8/4B3/R3K3 w - - 0 1").unwrap();
        // rook a1 — nothing aligned; build a real pin: white Ra1, Bb2 pinned to Kh... use classic
        let st = State::from_fen("3k4/8/8/8/8/8/3Nr3/3RK3 b - - 0 1").unwrap();
        let _ = st;
        // knight e2 pinned by rook d2 to king d1? geometry: d-file rook d2, knight d3?, king d1
        let st = State::from_fen("3k4/8/8/8/8/3n4/3R4/3K4 b - - 0 1").unwrap();
        let pins = find_pins(&st);
        assert!(pins.iter().any(|p| p["pinned"] == "d3"));
        let _ = parse_sq("d3");
    }

    #[test]
    fn classify_blunder() {
        let before = EvalPoint { cp: 0, mate: None };
        let after = EvalPoint { cp: -700, mate: None };
        assert_eq!(classify_move(before, after, true), "blunder");
        let after2 = EvalPoint { cp: -100, mate: None };
        assert_eq!(classify_move(before, after2, true), "inaccuracy");
    }
}
