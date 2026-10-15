//! PGN / database import. Handles:
//!  - .pgn (tag pairs + SAN movetext, comments, variations, NAGs, RPCN tokens)
//!  - .txt exports of chess.com games (raw JSON lines)
//!  - .jsonl (one game object per line with `moves` SAN or UCI string)
//!  - .csv (configurable columns; default white,black,result,elo_w,elo_b,utc_date,opening,moves)
//!  - .db/.sqlite3 game databases (games table with pgn or san column)
//! Robust to millions of games via streaming cursor API (`PgnCursor`).

use crate::board::{State, Move};
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Game {
    pub tags: Vec<(String, String)>,
    /// SAN moves (normalized when possible).
    pub moves: Vec<String>,
    #[serde(default)]
    pub start_fen: Option<String>,
    #[serde(skip)]
    pub raw_pgn: Option<String>,
}

impl Game {
    pub fn tag(&self, k: &str) -> &str {
        self.tags.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    pub fn summary(&self) -> GameSummary {
        let result = self.tag("Result");
        let (w, b) = if result == "1-0" {
            (1.0, 0.0)
        } else if result == "0-1" {
            (0.0, 1.0)
        } else if result == "1/2-1/2" {
            (0.5, 0.5)
        } else {
            (0.0, 0.0)
        };
        GameSummary {
            white: self.tag("White").to_string(),
            black: self.tag("Black").to_string(),
            white_elo: self.tag("WhiteElo").parse().unwrap_or(0),
            black_elo: self.tag("BlackElo").parse().unwrap_or(0),
            utc_date: self.tag("UTCDate").to_string(),
            result: result.to_string(),
            eco: self.tag("ECO").to_string(),
            opening: self.tag("Opening").to_string(),
            termination: self.tag("Termination").to_string(),
            num_moves: self.moves.len() / 2 + self.moves.len() % 2,
            score: w - b, // from white's perspective
        }
    }

    /// Replay the game; returns (final state, list of (san, uci, fen-before)).
    pub fn replay(&self) -> Vec<Ply> {
        let mut st = self
            .start_fen
            .as_deref()
            .and_then(State::from_fen)
            .unwrap_or_else(State::start);
        let mut out = Vec::with_capacity(self.moves.len());
        for san in &self.moves {
            let m = match st.find_move_san(san).or_else(|| st.find_move_uci(san)) {
                Some(m) => m,
                None => break,
            };
            out.push(Ply {
                san: st.san(m),
                uci: uci_string(m),
                before: st.to_fen(),
                key: st.key(),
            });
            st = st.make(m);
        }
        out
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Ply {
    pub san: String,
    pub uci: String,
    pub before: String,
    pub key: String,
}

pub fn uci_string(m: Move) -> String {
    use crate::board::sq_name;
    let promo = match m.promo {
        crate::board::QUEEN => "q",
        crate::board::ROOK => "r",
        crate::board::BISHOP => "b",
        crate::board::KNIGHT => "n",
        _ => "",
    };
    format!("{}{}{}", sq_name(m.from), sq_name(m.to), promo)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameSummary {
    pub white: String,
    pub black: String,
    pub white_elo: i32,
    pub black_elo: i32,
    pub utc_date: String,
    pub result: String,
    pub eco: String,
    pub opening: String,
    pub termination: String,
    pub num_moves: usize,
    pub score: f64,
}

// ---------------------------------------------------------------------------
// PGN tokenizer / parser
// ---------------------------------------------------------------------------

fn is_movetext_token(tok: &str) -> bool {
    if tok.is_empty() {
        return false;
    }
    let first = tok.as_bytes()[0];
    // SAN-ish starts
    let ok_start = first.is_ascii_alphabetic() || first == b'0' || first == b'O' || first == b'*';
    // strip trailing annotations/glyphs for checks like e4!?, Nxd5+
    let core = tok.trim_end_matches(['!', '?', '+', '#', '!', ' ', '\u{00BD}']);
    if core.is_empty() {
        return false;
    }
    let has_digit = core.bytes().any(|b| b.is_ascii_digit());
    let all_move_chars = core
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'=' || b == b'+');
    ok_start && all_move_chars && (has_digit || core.starts_with('O') || core.starts_with('o'))
}

/// Extract movetext SAN tokens from a PGN movetext block (after tags).
pub fn parse_movetext(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    let n = bytes.len();
    let mut depth = 0i32;
    while i < n {
        let c = bytes[i] as char;
        match c {
            '{' => {
                // comment until matching }
                while i < n && bytes[i] as char != '}' {
                    i += 1;
                }
                i += 1;
            }
            ';' => {
                while i < n && bytes[i] as char != '\n' {
                    i += 1;
                }
            }
            '(' => {
                depth += 1;
                i += 1;
            }
            ')' => {
                if depth > 0 {
                    depth -= 1;
                }
                i += 1;
            }
            _ if c.is_whitespace() => i += 1,
            _ => {
                let start = i;
                while i < n && !(bytes[i] as char).is_whitespace() && !"{();}".contains(bytes[i] as char) {
                    i += 1;
                }
                let tok = &text[start..i];
                if depth == 0 && is_movetext_token(tok) {
                    // skip move numbers like "12." or "12..."
                    let t = tok.trim_end_matches('.');
                    if !t.is_empty() && !t.chars().all(|ch| ch.is_ascii_digit()) {
                        out.push(tok.trim_end_matches(['!', '?']).trim_end_matches(['!']).to_string());
                    }
                }
            }
        }
    }
    out
}

/// Parse a full PGN string into one game (first game if several).
pub fn parse_pgn(text: &str) -> Option<Game> {
    parse_pgn_all(text).into_iter().next()
}

/// Parse many games from one PGN text (streaming-friendly for moderate sizes).
pub fn parse_pgn_all(text: &str) -> Vec<Game> {
    let mut games = Vec::new();
    let mut cur_tags: Vec<(String, String)> = Vec::new();
    let mut movetext = String::new();
    let mut in_movetext = false;
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("[") && l.contains("]") && !in_movetext {
            parse_tag_line(l, &mut cur_tags);
        } else if l.is_empty() {
            // blank line: could separate games only if we already have content
            if !cur_tags.is_empty() && !movetext.trim().is_empty() {
                // keep collecting; games normally separated by header blocks
            }
        } else if l.starts_with("[") && in_movetext {
            // new game headers without blank line separation
            finish_game(&mut games, &mut cur_tags, &mut movetext, &mut in_movetext);
            parse_tag_line(l, &mut cur_tags);
        } else {
            in_movetext = true;
            movetext.push_str(l);
            movetext.push('\n');
        }
    }
    if !cur_tags.is_empty() || !movetext.trim().is_empty() {
        finish_game(&mut games, &mut cur_tags, &mut movetext, &mut in_movetext);
    }
    games
}

fn parse_tag_line(l: &str, tags: &mut Vec<(String, String)>) {
    // [Key "Value"] possibly multiple pairs on one line.
    // Scan pair-by-pair (regex-like manual walk) so a greedy rfind(']') can't
    // swallow several pairs into one and a first-quote/last-quote mismatch can't
    // merge two values into one.
    let bytes = l.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let start = i + 1;
        // find end of this tag pair: the ']' that follows the closing '"'
        let mut j = start;
        let mut in_str = false;
        while j < bytes.len() {
            match bytes[j] {
                b'"' => in_str = !in_str,
                b']' if !in_str => break,
                _ => {}
            }
            j += 1;
        }
        if j >= bytes.len() {
            break; // unterminated tag line
        }
        let inner = &l[start..j];
        if let Some(q1) = inner.find('"') {
            if let Some(q2) = inner[q1 + 1..].rfind('"') {
                let q2 = q1 + 1 + q2;
                let key = inner[..q1].trim().to_string();
                let val = inner[q1 + 1..q2].to_string();
                if !key.is_empty() {
                    tags.retain(|(k, _)| k != &key);
                    tags.push((key, val));
                }
            }
        }
        i = j + 1;
    }
}

fn finish_game(games: &mut Vec<Game>, tags: &mut Vec<(String, String)>, movetext: &mut String, in_movetext: &mut bool) {
    let sans = parse_movetext(movetext);
    if !sans.is_empty() || !tags.is_empty() {
        let start_fen = tags
            .iter()
            .find(|(k, _)| k == "FEN" || k == "StartFEN")
            .map(|(_, v)| v.clone());
        games.push(Game {
            tags: std::mem::take(tags),
            moves: sans,
            start_fen,
            raw_pgn: None,
        });
    }
    tags.clear();
    movetext.clear();
    *in_movetext = false;
}

// ---------------------------------------------------------------------------
// Streaming cursor over files of many formats
// ---------------------------------------------------------------------------

pub struct PgnCursor<R: Read> {
    reader: R,
    buf: Vec<u8>,
    pos: usize,
    eof: bool,
    fmt: FileFormat,
    line_no: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileFormat {
    Pgn,
    Jsonl,
    Csv,
    Sqlite,
    Unknown,
}

pub fn detect_format(path: &str) -> FileFormat {
    let p = path.to_ascii_lowercase();
    if p.ends_with(".sqlite") || p.ends_with(".sqlite3") || p.ends_with(".db") || p.ends_with(".cbh") {
        FileFormat::Sqlite
    } else if p.ends_with(".jsonl") || p.ends_with(".json") || p.ends_with(".ndjson") {
        FileFormat::Jsonl
    } else if p.ends_with(".csv") || p.ends_with(".tsv") {
        FileFormat::Csv
    } else if p.ends_with(".pgn") || p.ends_with(".txt") || p.ends_with(".pgn.gz") {
        FileFormat::Pgn
    } else {
        FileFormat::Unknown
    }
}

impl PgnCursor<std::fs::File> {
    pub fn open(path: &str) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let fmt = detect_format(path);
        Ok(Self {
            reader: file,
            buf: Vec::with_capacity(1 << 20),
            pos: 0,
            eof: false,
            fmt,
            line_no: 0,
        })
    }

    pub fn next_game(&mut self) -> Option<Game> {
        match self.fmt {
            FileFormat::Pgn => self.next_pgn_game(),
            FileFormat::Jsonl => self.next_jsonl_game(),
            FileFormat::Csv => self.next_csv_game(),
            _ => self.next_pgn_game(),
        }
    }

    fn fill(&mut self) -> bool {
        if self.eof {
            return self.pos < self.buf.len();
        }
        if self.pos > (1 << 20) {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        let mut chunk = [0u8; 1 << 16];
        match self.reader.read(&mut chunk) {
            Ok(0) => {
                self.eof = true;
                self.pos < self.buf.len()
            }
            Ok(k) => {
                self.buf.extend_from_slice(&chunk[..k]);
                true
            }
            Err(_) => {
                self.eof = true;
                false
            }
        }
    }

    /// Read one logical line (returns None at EOF).
    fn read_line(&mut self) -> Option<String> {
        loop {
            if let Some(nl) = self.buf[self.pos..].iter().position(|&b| b == b'\n') {
                let end = self.pos + nl;
                let line = String::from_utf8_lossy(&self.buf[self.pos..end]).to_string();
                self.pos = end + 1;
                self.line_no += 1;
                return Some(line);
            }
            if !self.fill() {
                if self.pos < self.buf.len() {
                    let line = String::from_utf8_lossy(&self.buf[self.pos..]).to_string();
                    self.pos = self.buf.len();
                    self.line_no += 1;
                    return Some(line);
                }
                return None;
            }
        }
    }

    fn next_pgn_game(&mut self) -> Option<Game> {
        // accumulate until blank-line-separated header+movetext block ends
        let mut tags = Vec::new();
        let mut movetext = String::new();
        let mut got_any = false;
        let mut blank_seen = false;
        loop {
            let had_data = self.pos < self.buf.len() || !self.eof;
            let line = match self.read_line() {
                Some(l) => l,
                None => break,
            };
            let l = line.trim_end_matches('\r');
            let t = l.trim();
            if t.is_empty() {
                if got_any && !movetext.trim().is_empty() {
                    // blank line after movetext may end the game — but PGN allows blank lines inside? rare.
                    // Peek: if next non-empty line starts with '[' and we have movetext, new game.
                    self.pushback_blank_and_check(&mut tags, &mut movetext, &mut got_any);
                    if got_any {
                        // fallthrough handled below
                    }
                }
                blank_seen = true;
                continue;
            }
            if t.starts_with("[") {
                if got_any && blank_seen && !movetext.trim().is_empty() {
                    // new game starting: rewind not possible; emit current
                    let g = build_game(&tags, &movetext);
                    tags.clear();
                    movetext.clear();
                    parse_tag_line(t, &mut tags);
                    return Some(g);
                }
                parse_tag_line(t, &mut tags);
                got_any = true;
            } else {
                movetext.push_str(t);
                movetext.push('\n');
                got_any = true;
            }
            let _ = had_data;
            blank_seen = false;
        }
        if got_any {
            Some(build_game(&tags, &movetext))
        } else {
            None
        }
    }

    fn pushback_blank_and_check(&mut self, _tags: &mut Vec<(String, String)>, _movetext: &mut String, _got_any: &mut bool) {
        // no-op: kept for clarity; game boundary detection happens on next '[' line
    }

    fn next_jsonl_game(&mut self) -> Option<Game> {
        while let Some(line) = self.read_line() {
            let t = line.trim();
            if t.is_empty() || !t.starts_with('{') {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(t) {
                return json_to_game(&v);
            }
        }
        None
    }

    fn next_csv_game(&mut self) -> Option<Game> {
        // simple CSV: header row defines columns; expects a moves column (SAN space-separated)
        let mut headers: Option<Vec<String>> = None;
        loop {
            let line = self.read_line()?;
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            let cells = split_csv_pub(t);
            if headers.is_none() {
                headers = Some(cells.iter().map(|s| s.to_ascii_lowercase()).collect());
                continue;
            }
            let h = headers.as_ref().unwrap();
            let mut tags = Vec::new();
            let mut moves_sans: Vec<String> = Vec::new();
            for (i, c) in cells.iter().enumerate() {
                let name = h.get(i).cloned().unwrap_or_default();
                match name.as_str() {
                    "white" | "player_w" | "w" => tags.push(("White".into(), c.to_string())),
                    "black" | "player_b" | "b" => tags.push(("Black".into(), c.to_string())),
                    "result" => tags.push(("Result".into(), c.to_string())),
                    "elo_w" | "whiteelo" | "white_elo" => tags.push(("WhiteElo".into(), c.to_string())),
                    "elo_b" | "blackelo" | "black_elo" => tags.push(("BlackElo".into(), c.to_string())),
                    "date" | "utc_date" | "utcdate" => tags.push(("UTCDate".into(), c.to_string())),
                    "eco" | "opening" | "event" => {}
                    "moves" | "san" | "pgn" => {
                        if name == "pgn" {
                            let g = parse_pgn(c);
                            if let Some(mut g) = g {
                                g.tags.extend(tags);
                                return Some(g);
                            }
                        } else {
                            moves_sans = c.split_whitespace().map(|s| s.to_string()).collect();
                        }
                    }
                    _ => {}
                }
            }
            if !moves_sans.is_empty() {
                return Some(Game {
                    tags,
                    moves: moves_sans,
                    start_fen: None,
                    raw_pgn: None,
                });
            }
        }
    }
}

fn build_game(tags: &[(String, String)], movetext: &str) -> Game {
    let start_fen = tags
        .iter()
        .find(|(k, _)| k == "FEN" || k == "StartFEN")
        .map(|(_, v)| v.clone());
    Game {
        tags: tags.to_vec(),
        moves: parse_movetext(movetext),
        start_fen,
        raw_pgn: None,
    }
}

pub fn split_csv_pub(s: &str) -> Vec<String> {
    // handles quoted fields with commas
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    for ch in s.chars() {
        match ch {
            '"' => in_q = !in_q,
            ',' if !in_q => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    out.push(cur.trim().to_string());
    out
}

fn json_to_game(v: &serde_json::Value) -> Option<Game> {
    // chess.com export style: {"moveList":[{"san":"e4",...}], ...}
    if let Some(ml) = v.get("moveList").and_then(|m| m.as_array()) {
        let moves: Vec<String> = ml.iter().filter_map(|m| m.get("san").and_then(|s| s.as_str()).map(|s| s.to_string())).collect();
        let player = |side: &str| -> String {
            v.get("players")
                .and_then(|p| p.get(side))
                .and_then(|p| p.get("username"))
                .and_then(|s| s.as_str())
                .unwrap_or("?")
                .to_string()
        };
        let elo = |side: &str| -> i64 {
            v.get("players")
                .and_then(|p| p.get(side))
                .and_then(|p| p.get("rating"))
                .and_then(|s| s.as_i64())
                .unwrap_or(0)
        };
        let ts = v.get("time_control").and_then(|t| t.get("time_window")).and_then(|s| s.as_i64());
        let date = ts.map(|t| {
            let secs = t / 1000;
            let (y, mo, d) = days_to_ymd(secs / 86400);
            format!("{:04}.{:02}.{:02}", y, mo, d)
        });
        let mut tags = vec![
            ("White".into(), player("white")),
            ("Black".into(), player("black")),
            ("WhiteElo".into(), elo("white").to_string()),
            ("BlackElo".into(), elo("black").to_string()),
            ("Result".into(), match v.get("result").and_then(|r| r.as_str()) {
                Some("win") => "1-0".to_string(),
                Some("loss") => "0-1".to_string(),
                Some("draw") => "1/2-1/2".to_string(),
                _ => "*".to_string(),
            }),
        ];
        if let Some(d) = date {
            tags.push(("UTCDate".into(), d));
        }
        return Some(Game {
            tags,
            moves,
            start_fen: None,
            raw_pgn: None,
        });
    }
    // generic: {"white":..,"black":..,"moves":"e4 e5 .."} or {"moves":["e4","e5"]}
    let get_s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    let moves = match v.get("moves") {
        Some(serde_json::Value::String(s)) => s.split_whitespace().map(|x| x.to_string()).collect(),
        Some(serde_json::Value::Array(a)) => a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect(),
        _ => return None,
    };
    let mut tags = Vec::new();
    if let Some(w) = get_s("white").or_else(|| get_s("White")) {
        tags.push(("White".into(), w));
    }
    if let Some(b) = get_s("black").or_else(|| get_s("Black")) {
        tags.push(("Black".into(), b));
    }
    if let Some(r) = get_s("result").or_else(|| get_s("Result")) {
        tags.push(("Result".into(), r));
    }
    if let Some(e) = get_s("white_elo") {
        tags.push(("WhiteElo".into(), e));
    }
    if let Some(e) = get_s("black_elo") {
        tags.push(("BlackElo".into(), e));
    }
    if let Some(d) = get_s("date").or_else(|| get_s("utc_date")) {
        tags.push(("UTCDate".into(), d));
    }
    Some(Game {
        tags,
        moves,
        start_fen: get_s("fen"),
        raw_pgn: None,
    })
}

fn days_to_ymd(days: i64) -> (i64, i64, i64) {
    // civil_from_days (Howard Hinnant)
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ---------------------------------------------------------------------------
// SQLite game-database import
// ---------------------------------------------------------------------------

pub fn import_sqlite_games(path: &str, conn: &rusqlite::Connection) -> rusqlite::Result<Vec<Game>> {
    let _ = path;
    let mut stmt = conn.prepare("SELECT pgn FROM games")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut games = Vec::new();
    for row in rows {
        if let Some(g) = parse_pgn(&row?) {
            games.push(g);
        }
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgn_basic() {
        let pgn = "[Event \"rated\"]\n[White \"Alice\"]\n[Black \"Bob\"]\n[Result \"1-0\"]\n\n1. e4 {comment} e5 2. Nf3 (2. d4 exd4) Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 d6 8. c3 O-O 9. h3 Nb8 10. d4 Nbd7 11. c4 c6 12. cxb5 axb5 13. Nc3 Bb7 14. Bg5 b4 15. Nb1 h6 16. Bh4 c5 17. dxe5 Nxe4 18. exd6 Nxd6 19. Nbd2 Bxh4 20. Nxh4 Qf6 21. Nhf3 Bd5 22. Bxd5 Nxc4 23. Bxa8 Qb2 24. Qc1 Bxf7 25. Re2 1-0";
        let g = parse_pgn(pgn).unwrap();
        assert_eq!(g.tag("White"), "Alice");
        assert_eq!(g.moves[0], "e4");
        assert_eq!(g.moves[1], "e5");
        assert!(g.moves.contains(&"O-O".to_string()));
        let plies = g.replay();
        assert!(plies.len() > 40);
        assert_eq!(plies[0].uci, "e2e4");
    }

    #[test]
    fn multi_game_stream() {
        let text = "[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 e5 *\n\n[White \"C\"]\n[Black \"D\"]\n[Result \"*\"]\n\n1. d4 d5 *";
        let games = parse_pgn_all(text);
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].tag("White"), "A");
        assert_eq!(games[1].tag("White"), "C");
    }

    #[test]
    fn rpcn_tokens_skipped() {
        let pgn = "[White \"A\"]\n\n1. e4 e5 2. Nf3 Nc6 3. Bb5 Nf6 4. O-O *  [Result \"*\"]";
        let g = parse_pgn(pgn).unwrap();
        assert_eq!(g.moves.len(), 7);
    }

    #[test]
    fn chesscom_json() {
        let line = r#"{"url":"https://chess.com/game","pgn":"","time_control":{"time_window":1700000000000},"result":"win","players":{"white":{"username":"magus","rating":1500},"black":{"username":"noob","rating":1400}},"moveList":[{"san":"e4"},{"san":"e5"},{"san":"Nf3"}]}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let g = json_to_game(&v).unwrap();
        assert_eq!(g.tag("White"), "magus");
        assert_eq!(g.moves.len(), 3);
        assert_eq!(g.tag("Result"), "1-0");
    }
}
