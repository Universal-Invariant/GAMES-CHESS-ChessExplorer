//! Board representation and move generation (castling, en passant, promotion).
//! Squares: 0..63, a8=0 .. h1=63 (little-endian rank-major: sq = (7-rank)*8 + file).

use serde::{Deserialize, Serialize};

pub const EMPTY: u8 = 0;
pub const PAWN: u8 = 1;
pub const KNIGHT: u8 = 2;
pub const BISHOP: u8 = 3;
pub const ROOK: u8 = 4;
pub const QUEEN: u8 = 5;
pub const KING: u8 = 6;

pub fn color_of(p: u8) -> u8 {
    if p == 0 {
        2
    } else if p & 8 != 0 {
        0
    } else {
        1
    }
} // 0 white, 1 black, 2 none

pub fn is_white(p: u8) -> bool {
    p != 0 && p & 8 != 0
}
pub fn is_black(p: u8) -> bool {
    p != 0 && p & 8 == 0 && p != 0
}
pub fn piece_type(p: u8) -> u8 {
    p & 7
}


mod board_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    pub fn serialize<S: Serializer>(v: &[u8; 64], s: S) -> Result<S::Ok, S::Error> {
        v.to_vec().serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 64], D::Error> {
        let v = <Vec<u8>>::deserialize(d)?;
        let mut a = [0u8; 64];
        if v.len() == 64 {
            a.copy_from_slice(&v);
        }
        Ok(a)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub promo: u8, // piece type or 0
    pub flag: u8, // 0 normal, 1 castleK, 2 castleQ, 3 ep-capture
}

pub const F_CASTLE_K: u8 = 1;
pub const F_CASTLE_Q: u8 = 2;
pub const F_EP: u8 = 3;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct State {
    #[serde(with = "board_serde")]
    pub board: [u8; 64],
    pub side: u8, // 0 white, 1 black
    pub castling: u8, // bits: 1 wK, 2 wQ, 4 bK, 8 bQ
    pub ep: i8, // target square or -1
    pub halfmove: u8,
    pub fullmove: u16,
}

impl Default for State {
    fn default() -> Self {
        Self::start()
    }
}

pub fn sq(file: u8, rank: u8) -> u8 {
    // Canonical layout: index increases downward, a8=0 .. h1=63. Chess rank r -> row (7-r).
    (7 - rank) * 8 + file
}
pub fn file_of(s: u8) -> u8 {
    s % 8
}
pub fn rank_of(s: u8) -> u8 {
    7 - s / 8 // chess rank (0-based) of index; row 0 => rank 8 => returns 7
}

pub fn sq_name(s: u8) -> String {
    format!("{}{}", (b'a' + file_of(s)) as char, rank_of(s) + 1)
}

pub fn piece_letter(p: u8) -> String {
    if p == 0 {
        return "-".into();
    }
    let ch = match piece_type(p) {
        PAWN => 'p',
        KNIGHT => 'n',
        BISHOP => 'b',
        ROOK => 'r',
        QUEEN => 'q',
        _ => 'k',
    };
    ch.to_string()
}

pub fn parse_sq(s: &str) -> Option<u8> {
    let b = s.as_bytes();
    if b.len() != 2 {
        return None;
    }
    let f = b[0].wrapping_sub(b'a');
    let r = b[1].wrapping_sub(b'1');
    if f > 7 || r > 7 {
        return None;
    }
    Some(sq(f, r))
}

impl State {
    pub fn start() -> Self {
        let mut board = [0u8; 64];
        let back = [ROOK, KNIGHT, BISHOP, QUEEN, KING, BISHOP, KNIGHT, ROOK];
        for f in 0..8 {
            board[f as usize] = back[f as usize];          // black pieces, rank 8 (row 0)
            board[8 + f as usize] = PAWN;                  // black pawns, rank 7 (row 1)
            board[48 + f as usize] = PAWN | 8;             // white pawns, rank 2 (row 6)
            board[56 + f as usize] = back[f as usize] | 8; // white pieces, rank 1 (row 7)
        }
        State {
            board,
            side: 0,
            castling: 15,
            ep: -1,
            halfmove: 0,
            fullmove: 1,
        }
    }

    pub fn from_fen(fen: &str) -> Option<State> {
        let parts: Vec<&str> = fen.split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }
        let mut board = [0u8; 64];
        let mut rank = 7i8; // FEN first row is rank 8
        let mut file = 0i8;
        for ch in parts[0].chars() {
            match ch {
                '/' => {
                    rank -= 1;
                    file = 0;
                }
                '1'..='8' => file += ch as i8 - '0' as i8,
                _ => {
                    let pt = match ch.to_ascii_lowercase() {
                        'p' => PAWN,
                        'n' => KNIGHT,
                        'b' => BISHOP,
                        'r' => ROOK,
                        'q' => QUEEN,
                        'k' => KING,
                        _ => return None,
                    };
                    let white = ch.is_ascii_uppercase();
                    if rank < 0 || file > 7 {
                        return None;
                    }
                    board[((7 - rank) * 8 + file) as usize] = pt | if white { 8 } else { 0 }; // FEN first row = rank 8 = rows 0
                    file += 1;
                }
            }
        }
        let mut st = State {
            board,
            side: if parts.get(1) == Some(&"b") { 1 } else { 0 },
            castling: 0,
            ep: -1,
            halfmove: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            fullmove: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(1),
        };
        if let Some(c) = parts.get(2) {
            for ch in c.chars() {
                match ch {
                    'K' => st.castling |= 1,
                    'Q' => st.castling |= 2,
                    'k' => st.castling |= 4,
                    'q' => st.castling |= 8,
                    _ => {}
                }
            }
        }
        if let Some(e) = parts.get(3) {
            if *e != "-" {
                st.ep = parse_sq(e).map(|s| s as i8).unwrap_or(-1);
            }
        }
        Some(st)
    }

    pub fn to_fen(&self) -> String {
        let mut out = String::new();
        // FEN lists ranks top-down starting from rank 8: row index i = 7-rank.
        for i in 0..8usize {
            let mut empty = 0;
            for f in 0..8 {
                let p = self.board[i * 8 + f];
                if p == 0 {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    out.push(char::from(b'0' + empty));
                    empty = 0;
                }
                let ch = match piece_type(p) {
                    PAWN => 'p',
                    KNIGHT => 'n',
                    BISHOP => 'b',
                    ROOK => 'r',
                    QUEEN => 'q',
                    _ => 'k',
                };
                out.push(if is_white(p) { ch.to_ascii_uppercase() } else { ch });
            }
            if empty > 0 {
                out.push(char::from(b'0' + empty));
            }
            if i < 7 {
                out.push('/');
            }
        }
        out.push_str(if self.side == 0 { " w" } else { " b" });
        out.push(' ');
        let mut cs = String::new();
        if self.castling & 1 != 0 {
            cs.push('K');
        }
        if self.castling & 2 != 0 {
            cs.push('Q');
        }
        if self.castling & 4 != 0 {
            cs.push('k');
        }
        if self.castling & 8 != 0 {
            cs.push('q');
        }
        out.push_str(&if cs.is_empty() { "-".to_string() } else { cs });
        out.push(' ');
        out.push_str(&if self.ep >= 0 { sq_name(self.ep as u8) } else { "-".to_string() });
        write_half_full(&mut out, self.halfmove, self.fullmove);
        out
    }

    pub fn key(&self) -> String {
        // position key for repetition/epd: board + side + castling + ep
        let mut out = String::new();
        // Position key uses FEN-like top-down rank order.
        for i in 0..8usize {
            let mut empty = 0;
            for f in 0..8 {
                let p = self.board[i * 8 + f];
                if p == 0 {
                    empty += 1;
                    continue;
                }
                if empty > 0 {
                    out.push(char::from(b'0' + empty));
                    empty = 0;
                }
                let ch = match piece_type(p) {
                    PAWN => 'p',
                    KNIGHT => 'n',
                    BISHOP => 'b',
                    ROOK => 'r',
                    QUEEN => 'q',
                    _ => 'k',
                };
                out.push(if is_white(p) { ch.to_ascii_uppercase() } else { ch });
            }
            if empty > 0 {
                out.push(char::from(b'0' + empty));
            }
            if i < 7 {
                out.push('/');
            }
        }
        out.push(if self.side == 0 { 'w' } else { 'b' });
        out
    }

    pub fn attacked(&self, s: u8, by: u8) -> bool {
        // is square s attacked by color `by`?
        let f = file_of(s);
        let r = rank_of(s);
        // Layout: a8=0..h1=63, chess rank r -> row (7-r). White advances toward rank 8.
        // A white pawn attacking square s sits one rank BELOW s (rank_of(s)-1).
        let pd: i8 = if by == 0 { -1 } else { 1 }; // delta in CHESS RANK of the attacker square
        for df in [-1i8, 1] {
            let nf = f as i8 + df;
            let nr = r as i8 + pd;
            if nf >= 0 && nf < 8 && nr >= 0 && nr < 8 {
                let p = self.board[(sq(nf as u8, nr as u8)) as usize];
                if piece_type(p) == PAWN && color_of(p) == by {
                    return true;
                }
            }
        }
        // knights
        for (df, dr) in KNIGHT_DELTAS {
            if let Some(p) = self.at_delta(s, df, dr) {
                if piece_type(p) == KNIGHT && color_of(p) == by {
                    return true;
                }
            }
        }
        // king
        for df in -1i8..=1 {
            for dr in -1i8..=1 {
                if df == 0 && dr == 0 {
                    continue;
                }
                if let Some(p) = self.at_delta(s, df, dr) {
                    if piece_type(p) == KING && color_of(p) == by {
                        return true;
                    }
                }
            }
        }
        // sliders
        for (df, dr) in SLIDER_DELTAS {
            let diag = df != 0 && dr != 0;
            let mut cf = f as i8 + df;
            let mut cr = r as i8 + dr;
            while cf >= 0 && cf < 8 && cr >= 0 && cr < 8 {
                let p = self.board[(sq(cf as u8, cr as u8)) as usize];
                if p != 0 {
                    let t = piece_type(p);
                    if color_of(p) == by
                        && ((diag && (t == BISHOP || t == QUEEN)) || (!diag && (t == ROOK || t == QUEEN)))
                    {
                        return true;
                    }
                    break;
                }
                cf += df;
                cr += dr;
            }
        }
        false
    }

    fn at_delta(&self, s: u8, df: i8, dr: i8) -> Option<u8> {
        let nf = file_of(s) as i8 + df;
        let nr = rank_of(s) as i8 + dr;
        if nf < 0 || nf > 7 || nr < 0 || nr > 7 {
            return None;
        }
        Some(self.board[(sq(nf as u8, nr as u8)) as usize])
    }

    pub fn king_square(&self, side: u8) -> u8 {
        for s in 0..64u8 {
            let p = self.board[s as usize];
            if piece_type(p) == KING && color_of(p) == side {
                return s;
            }
        }
        0
    }

    pub fn in_check(&self, side: u8) -> bool {
        self.attacked(self.king_square(side), 1 - side)
    }

    pub fn generate_moves(&self) -> Vec<Move> {
        let mut mv = Vec::with_capacity(64);
        let us = self.side;
        let them = 1 - us;
        for s in 0..64u8 {
            let p = self.board[s as usize];
            if p == 0 || color_of(p) != us {
                continue;
            }
            let f = file_of(s);
            let r = rank_of(s);
            match piece_type(p) {
                PAWN => {
                    // Layout: a8=0..h1=63 (row-major top-down). White advances toward rank 8
                    // = rows decrease = index DECREASES (-8); black advances +8.
                    let step: i8 = if us == 0 { -8 } else { 8 };
                    let one = s as i8 + step;
                    if one >= 0 && one < 64 {
                        let t = one as u8;
                        if self.board[t as usize] == 0 {
                            // promotion when destination lands on the last row (idx range)
                            let promotes = if us == 0 { one < 8 } else { one >= 56 };
                            if promotes {
                                for pr in [QUEEN, ROOK, BISHOP, KNIGHT] {
                                    mv.push(Move { from: s, to: t, promo: pr, flag: 0 });
                                }
                            } else {
                                mv.push(Move { from: s, to: t, promo: 0, flag: 0 });
                                // double push only from home rank (index range check)
                                let home = if us == 0 { s >= 48 && s < 56 } else { s >= 8 && s < 16 };
                                if home {
                                    let two = one + step;
                                    if two >= 0 && two < 64 && self.board[two as usize] == 0 {
                                        mv.push(Move { from: s, to: two as u8, promo: 0, flag: 0 });
                                    }
                                }
                            }
                        }
                    }
                    // captures: target square is one step forward and one file sideways
                    for df in [-1i8, 1] {
                        let nf = f as i8 + df;
                        if nf < 0 || nf > 7 || one < 0 || one >= 64 {
                            continue;
                        }
                        let t = sq(nf as u8, rank_of(one as u8));
                        let tp = self.board[t as usize];
                        let promotes_cap = if us == 0 { one < 8 } else { one >= 56 };
                        if tp != 0 && color_of(tp) == them {
                            if promotes_cap {
                                for pr in [QUEEN, ROOK, BISHOP, KNIGHT] {
                                    mv.push(Move { from: s, to: t, promo: pr, flag: 0 });
                                }
                            } else {
                                mv.push(Move { from: s, to: t, promo: 0, flag: 0 });
                            }
                        } else if tp == 0 && self.ep >= 0 && self.ep as u8 == t {
                            mv.push(Move { from: s, to: t, promo: 0, flag: F_EP });
                        }
                    }
                }
                KNIGHT => {
                    for (df, dr) in KNIGHT_DELTAS {
                        if let Some(t) = self.target_delta(s, df, dr, them) {
                            mv.push(Move { from: s, to: t, promo: 0, flag: 0 });
                        }
                    }
                }
                BISHOP | ROOK | QUEEN => {
                    let t = piece_type(p);
                    // bishop: 4 diagonals (idx 4..8), rook: 4 orthogonals (0..4), queen: all 8
                    let (start, end) = if t == BISHOP {
                        (4, 8)
                    } else if t == ROOK {
                        (0, 4)
                    } else {
                        (0, 8)
                    };
                    for i in start..end {
                        let (dfs, drs) = SLIDER_DELTAS[i];
                        {
                            let mut cf = f as i8 + dfs;
                            let mut cr = r as i8 + drs;
                            while cf >= 0 && cf < 8 && cr >= 0 && cr < 8 {
                                let t = sq(cf as u8, cr as u8);
                                let tp = self.board[t as usize];
                                if tp == 0 {
                                    mv.push(Move { from: s, to: t, promo: 0, flag: 0 });
                                } else {
                                    if color_of(tp) == them {
                                        mv.push(Move { from: s, to: t, promo: 0, flag: 0 });
                                    }
                                    break;
                                }
                                cf += dfs;
                                cr += drs;
                            }
                        }
                    }
                }
                KING => {
                    for df in -1i8..=1 {
                        for dr in -1i8..=1 {
                            if df == 0 && dr == 0 {
                                continue;
                            }
                            if let Some(t) = self.target_delta(s, df, dr, them) {
                                mv.push(Move { from: s, to: t, promo: 0, flag: 0 });
                            }
                        }
                    }
                    // castling
                    if !self.in_check(us) {
                        let home_rank: u8 = if us == 0 { 7 } else { 0 };
                        let kbit = if us == 0 { 1 } else { 4 };
                        let qbit = if us == 0 { 2 } else { 8 };
                        let king_sq = sq(4, home_rank);
                        if self.board[king_sq as usize] == (KING | if us == 0 { 8 } else { 0 }) {
                            if self.castling & kbit != 0 {
                                // g,f empty; king transit squares e,f,g not attacked
                                let f1 = sq(5, home_rank);
                                let g1 = sq(6, home_rank);
                                let rook = sq(7, home_rank);
                                if self.board[f1 as usize] == 0
                                    && self.board[g1 as usize] == 0
                                    && piece_type(self.board[rook as usize]) == ROOK
                                    && color_of(self.board[rook as usize]) == us
                                    && !self.attacked(f1, them)
                                    && !self.attacked(g1, them)
                                {
                                    mv.push(Move { from: king_sq, to: g1, promo: 0, flag: F_CASTLE_K });
                                }
                            }
                            if self.castling & qbit != 0 {
                                // b,c,d empty; king transit squares c,d not attacked.
                                // (b1 may be attacked — the rook passes through it.)
                                let d1 = sq(3, home_rank);
                                let c1 = sq(2, home_rank);
                                let b1 = sq(1, home_rank);
                                let rook = sq(0, home_rank);
                                if self.board[d1 as usize] == 0
                                    && self.board[c1 as usize] == 0
                                    && self.board[b1 as usize] == 0
                                    && piece_type(self.board[rook as usize]) == ROOK
                                    && color_of(self.board[rook as usize]) == us
                                    && !self.attacked(d1, them)
                                    && !self.attacked(c1, them)
                                {
                                    mv.push(Move { from: king_sq, to: c1, promo: 0, flag: F_CASTLE_Q });
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        mv
    }

    fn target_delta(&self, s: u8, df: i8, dr: i8, them: u8) -> Option<u8> {
        let nf = file_of(s) as i8 + df;
        let nr = rank_of(s) as i8 + dr;
        if nf < 0 || nf > 7 || nr < 0 || nr > 7 {
            return None;
        }
        let t = sq(nf as u8, nr as u8);
        let tp = self.board[t as usize];
        if tp == 0 || color_of(tp) == them {
            Some(t)
        } else {
            None
        }
    }

    /// Legal moves only (king safety filtered).
    pub fn legal_moves(&self) -> Vec<Move> {
        let mut out = Vec::new();
        for m in self.generate_moves() {
            let ns = self.make(m);
            if !ns.in_check(self.side) {
                out.push(m);
            }
        }
        out
    }

    pub fn make(&self, m: Move) -> State {
        let mut ns = *self;
        let us = self.side;
        let them = 1 - us;
        let piece = ns.board[m.from as usize];
        let captured = if m.flag == F_EP {
            // Layout a8=0..h1=63: white advances -8, so the captured black pawn sits one row
            // BELOW (higher index) the ep target; for black it's one row above (lower index).
            let cap_sq = if us == 0 { m.to + 8 } else { m.to.wrapping_sub(8) };
            ns.board[cap_sq as usize]
        } else {
            ns.board[m.to as usize]
        };
        ns.board[m.from as usize] = 0;
        if m.flag == F_EP {
            // physically remove the captured pawn
            let cap_sq = if us == 0 { m.to + 8 } else { m.to.wrapping_sub(8) };
            ns.board[cap_sq as usize] = 0;
        }
        let placed = if m.promo != 0 { m.promo | if us == 0 { 8 } else { 0 } } else { piece };
        ns.board[m.to as usize] = placed;
        if m.flag == F_CASTLE_K {
            let hr = if us == 0 { 7u8 } else { 0u8 };
            ns.board[sq(7, hr) as usize] = 0;
            ns.board[sq(5, hr) as usize] = ROOK | if us == 0 { 8 } else { 0 };
        } else if m.flag == F_CASTLE_Q {
            let hr = if us == 0 { 7u8 } else { 0u8 };
            ns.board[sq(0, hr) as usize] = 0;
            ns.board[sq(3, hr) as usize] = ROOK | if us == 0 { 8 } else { 0 };
        }
        // castling rights updates
        if piece_type(piece) == KING {
            ns.castling &= if us == 0 { !3 } else { !12 };
        }
        for &(s, bit) in &[
            (sq(0, 7), 2u8),
            (sq(7, 7), 1u8),
            (sq(0, 0), 8u8),
            (sq(7, 0), 4u8),
        ] {
            if m.from == s || m.to == s {
                ns.castling &= !bit;
            }
        }
        // En-passant: `ep` stores the *target* square (behind the double-pushed pawn), FEN-style.
        ns.ep = if piece_type(piece) == PAWN && (m.to as i8 - m.from as i8).abs() == 16 {
            let passed = ((m.from + m.to) / 2) as i8;
            // FEN-style ep target: the square the pawn skipped over = midpoint of from/to.
            let target = passed;
            let ep_side = if us == 0 { 1u8 } else { 0u8 };
            if ns.attacked(target as u8, ep_side) { target } else { -1 }
        } else {
            -1
        };
        ns.halfmove = if piece_type(piece) == PAWN || captured != 0 { 0 } else { ns.halfmove + 1 };
        if us == 1 {
            ns.fullmove += 1;
        }
        ns.side = them;
        ns
    }

    pub fn san(&self, m: Move) -> String {
        let ns = self.make(m);
        let check = if ns.in_check(ns.side) { "+" } else { "" };
        let pt = piece_type(self.board[m.from as usize]);
        if m.flag == F_CASTLE_K {
            return format!("O-O{}", check);
        }
        if m.flag == F_CASTLE_Q {
            return format!("O-O-O{}", check);
        }
        if pt == PAWN {
            let capture = self.board[m.to as usize] != 0 || m.flag == F_EP;
            let mut s = String::new();
            if capture {
                s.push((b'a' + file_of(m.from)) as char);
                s.push('x');
            }
            s.push_str(&sq_name(m.to));
            if m.promo != 0 {
                s.push('=');
                s.push(match m.promo {
                    KNIGHT => 'N',
                    BISHOP => 'B',
                    ROOK => 'R',
                    _ => 'Q',
                });
            }
            format!("{}{}", s, check)
        } else {
            let letter = match pt {
                KNIGHT => 'N',
                BISHOP => 'B',
                ROOK => 'R',
                QUEEN => 'Q',
                _ => 'K',
            };
            // disambiguation
            let others: Vec<Move> = self
                .legal_moves()
                .into_iter()
                .filter(|o| {
                    o.to == m.to
                        && o.from != m.from
                        && piece_type(self.board[o.from as usize]) == pt
                })
                .collect();
            let mut dis = String::new();
            if !others.is_empty() {
                let same_file = others.iter().any(|o| file_of(o.from) == file_of(m.from));
                let same_rank = others.iter().any(|o| rank_of(o.from) == rank_of(m.from));
                if !same_file {
                    dis.push((b'a' + file_of(m.from)) as char);
                } else if !same_rank {
                    dis.push(char::from(b'1' + rank_of(m.from)));
                } else {
                    dis.push((b'a' + file_of(m.from)) as char);
                    dis.push(char::from(b'1' + rank_of(m.from)));
                }
            }
            let capture = self.board[m.to as usize] != 0;
            format!(
                "{}{}{}{}{}",
                letter,
                dis,
                if capture { "x" } else { "" },
                sq_name(m.to),
                check
            )
        }
    }

    pub fn find_move_san(&self, san: &str) -> Option<Move> {
        let san_clean = san.trim_end_matches(['+', '#', '!']);
        for m in self.legal_moves() {
            let s = self.san(m);
            let s_clean = s.trim_end_matches(['+', '#']);
            if s_clean == san_clean {
                return Some(m);
            }
        }
        None
    }

    pub fn find_move_uci(&self, uci: &str) -> Option<Move> {
        let b = uci.as_bytes();
        if b.len() < 4 {
            return None;
        }
        let from = parse_sq(&uci[0..2])?;
        let to = parse_sq(&uci[2..4])?;
        let promo = if b.len() > 4 {
            match uci.as_bytes()[4] {
                b'q' => QUEEN,
                b'r' => ROOK,
                b'b' => BISHOP,
                b'n' => KNIGHT,
                _ => 0,
            }
        } else {
            0
        };
        for m in self.legal_moves() {
            if m.from == from && m.to == to && (promo == 0 || m.promo == promo) {
                return Some(m);
            }
        }
        None
    }

    pub fn game_over(&self) -> Option<(String, &'static str)> {
        let moves = self.legal_moves();
        if moves.is_empty() {
            return if self.in_check(self.side) {
                Some(("checkmate".to_string(), if self.side == 0 { "0-1" } else { "1-0" }))
            } else {
                Some(("stalemate".to_string(), "1/2-1/2"))
            };
        }
        if self.halfmove >= 100 {
            return Some(("fifty-move rule".to_string(), "1/2-1/2"));
        }
        None
    }
}

const KNIGHT_DELTAS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

/// Orthogonal deltas first (0..4), diagonal deltas second (4..8).
pub const SLIDER_DELTAS: [(i8, i8); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

fn write_half_full(out: &mut String, hm: u8, fm: u16) {
    use std::fmt::Write;
    let _ = write!(out, " {} {}", hm, fm);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(state: &mut State, sans: &[&str]) {
        for san in sans {
            let m = state.find_move_san(san).unwrap_or_else(|| panic!("bad SAN {}", san));
            *state = state.make(m);
        }
    }

    #[test]
    fn perft_startpos() {
        let s = State::start();
        assert_eq!(perft(&s, 1), 20);
        assert_eq!(perft(&s, 2), 400);
        assert_eq!(perft(&s, 3), 8902);
        assert_eq!(perft(&s, 4), 197281);
    }

    #[test]
    fn dbg_kiwipete() {
        let s = State::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").unwrap();
        let pseudo = s.generate_moves();
        eprintln!("pseudo count {}", pseudo.len());
        for m in &pseudo {
            if m.flag != 0 {
                eprintln!("special: {} flag {} san {}", sq_name(m.to), m.flag, s.san(*m));
            }
        }
        let legal = s.legal_moves();
        eprintln!("legal count {}", legal.len());
        for m in &pseudo {
            if !legal.contains(m) {
                eprintln!(
                    "filtered: {}->{} flag {} promo {}",
                    sq_name(m.from),
                    sq_name(m.to),
                    m.flag,
                    m.promo
                );
            }
        }
        panic!("dbg");
    }

    #[test]
    fn perft_kiwipete() {
        let s = State::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1").unwrap();
        assert_eq!(perft(&s, 1), 48);
        assert_eq!(perft(&s, 2), 2039);
        assert_eq!(perft(&s, 3), 97862);
    }

    fn perft(s: &State, d: u8) -> u64 {
        if d == 0 {
            return 1;
        }
        let mut n = 0;
        for m in s.legal_moves() {
            let ns = s.make(m);
            n += if d == 1 { 1 } else { perft(&ns, d - 1) };
        }
        n
    }

    #[test]
    fn san_roundtrip_and_mate() {
        let mut s = State::start();
        play(&mut s, &["e4", "e5", "Nf3", "Nc6", "Bc4", "Nd4", "Nxe5", "Qg5", "Bxf7+", "Ke7", "Qh5", "g6", "Nxg6+", "hxg6", "Qxh8"]);
        assert_eq!(s.to_fen().split(' ').next().unwrap().len() > 0, true);
        let mut s2 = State::start();
        play(&mut s2, &["f3", "e5", "g4", "Qh4"]);
        assert!(s2.in_check(0));
        assert_eq!(s2.legal_moves().len(), 0);
    }

    #[test]
    fn fen_roundtrip() {
        let s = State::start();
        assert_eq!(s.to_fen(), "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1");
        let fen = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
        let s = State::from_fen(fen).unwrap();
        assert_eq!(s.to_fen(), fen);
    }
}
