//! Opening detection: ECO code + name from move sequence, via a prefix trie.
//! Ships with an embedded mini-database (~130 common lines). Users can load the
//! full Lichess `eco.py`-style list or any CSV/JSON opening file at runtime.

use crate::board::State;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Opening {
    pub eco: String,
    pub name: String,
    pub line: Vec<String>, // SAN moves from start position
}

#[derive(Default)]
struct Node {
    children: HashMap<String, Node>,
    opening: Option<(String, String)>, // (eco, name) — deepest match wins
}

#[derive(Default)]
pub struct OpeningDb {
    root: Node,
    count: usize,
}

impl OpeningDb {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, eco: &str, name: &str, sans: &[&str]) {
        let mut node = &mut self.root;
        for san in sans {
            node = node.children.entry(san.to_string()).or_default();
        }
        if node.opening.is_none() {
            self.count += 1;
        }
        node.opening = Some((eco.to_string(), name.to_string()));
    }

    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Longest-prefix match of a game's SAN moves.
    pub fn lookup(&self, sans: &[String]) -> Option<Opening> {
        let mut node = &self.root;
        let mut best: Option<(String, String)> = None;
        let mut depth = 0usize;
        for san in sans {
            match node.children.get(san) {
                Some(n) => {
                    node = n;
                    depth += 1;
                    if let Some(o) = &node.opening {
                        best = Some(o.clone());
                    }
                }
                None => break,
            }
        }
        best.map(|(eco, name)| Opening {
            eco,
            name,
            line: sans[..depth].to_vec(),
        })
    }

    /// Child branches from a given prefix (for explorer UI).
    pub fn children_of(&self, prefix: &[String]) -> Vec<(String, Option<Opening>)> {
        let mut node = &self.root;
        for san in prefix {
            match node.children.get(san) {
                Some(n) => node = n,
                None => return Vec::new(),
            }
        }
        node.children
            .iter()
            .map(|(san, n)| {
                (
                    san.clone(),
                    n.opening.as_ref().map(|(eco, name)| Opening {
                        eco: eco.clone(),
                        name: name.clone(),
                        line: Vec::new(),
                    }),
                )
            })
            .collect()
    }

    /// Embedded standard openings.
    pub fn embedded() -> OpeningDb {
        let mut db = OpeningDb::default();
        macro_rules! o {
            ($eco:expr, $name:expr, [$($mv:expr),*]) => { db.insert($eco, $name, &[$($mv),*]); };
        }
        o!("A00", "Barnes Opening", ["1. f3"]);
        o!("A00", "Grob", ["1. g4"]);
        o!("A00", "Polish/Sokolsky Opening", ["1. b4"]);
        o!("A02", "Bird's Opening", ["1. f4"]);
        o!("A04", "King's Indian Attack, Sicilian version", ["1. e4", "c5", "2. d3"]);
        o!("A07", "King's Indian Attack", ["1. e4", "e6", "2. d3", "d5", "3. Nd2"]);
        o!("A10", "English Opening", ["1. c4"]);
        o!("A13", "Neo-Catalan", ["1. c4", "e6", "2. Nf3", "d5", "3. g3"]);
        o!("A15", "English, Anglo-Slavic", ["1. c4", "Nf6", "2. Nf3", "c6"]);
        o!("A16", "English, Anglo-Grünfeld", ["1. c4", "Nf6", "2. Nc3", "d5"]);
        o!("A20", "English Opening, Reversed Sicilian", ["1. c4", "e5"]);
        o!("A30", "English, Symmetrical variation", ["1. c4", "c5"]);
        o!("A40", "Queen's Pawn Game", ["1. d4", "e6"]);
        o!("A42", "Modern Defense, Averbakh System", ["1. d4", "g6", "2. c4", "Bg7", "3. Nc3", "d6", "4. e4"]);
        o!("A43", "Old Benoni Defense", ["1. d4", "c5"]);
        o!("A45", "Trompowsky Attack", ["1. d4", "Nf6", "2. Bg5"]);
        o!("A46", "London System", ["1. d4", "Nf6", "2. Bf4"]);
        o!("A48", "East Indian Defense", ["1. d4", "Nf6", "2. Nf3", "g6"]);
        o!("A50", "Queen's Indian Accelerated", ["1. d4", "Nf6", "2. c4", "b6"]);
        o!("A56", "Benoni Defense", ["1. d4", "Nf6", "2. c4", "c5", "3. d5"]);
        o!("A57", "Volga/Benko Gambit", ["1. d4", "Nf6", "2. c4", "c5", "3. d5", "b5"]);
        o!("A58", "Benko Gambit Accepted", ["1. d4", "Nf6", "2. c4", "c5", "3. d5", "b5", "4. cxb5", "a6"]);
        o!("A60", "Benoni Defense, Modern variation", ["1. d4", "Nf6", "2. c4", "c5", "3. d5", "e6"]);
        o!("A80", "Dutch Defense", ["1. d4", "f5"]);
        o!("A85", "Dutch, Leningrad Variation", ["1. d4", "f5", "2. c4", "Nf6", "3. Nc3", "g6"]);
        o!("B00", "Nimzowitsch Defense", ["1. e4", "Nc6"]);
        o!("B01", "Scandinavian Defense", ["1. e4", "d5"]);
        o!("B02", "Alekhine's Defense", ["1. e4", "Nf6"]);
        o!("B06", "Modern Defense", ["1. e4", "g6"]);
        o!("B07", "Pirc Defense", ["1. e4", "d6", "2. d4", "Nf6"]);
        o!("B09", "Pirc, Austrian Attack", ["1. e4", "d6", "2. d4", "Nf6", "3. Nc3", "g6", "4. f4"]);
        o!("B10", "Caro-Kann Defense", ["1. e4", "c6"]);
        o!("B12", "Caro-Kann, Advance Variation", ["1. e4", "c6", "2. d4", "d5", "3. e5"]);
        o!("B15", "Caro-Kann, Two Knights System", ["1. e4", "c6", "2. d4", "d5", "3. Nc3", "dxe4", "4. Nxe4"]);
        o!("B20", "Sicilian Defense", ["1. e4", "c5"]);
        o!("B21", "Sicilian, Smith-Morra-Gambit", ["1. e4", "c5", "2. d4", "cxd4", "3. c3"]);
        o!("B22", "Sicilian, Alapin Variation", ["1. e4", "c5", "2. c3"]);
        o!("B23", "Sicilian, Closed", ["1. e4", "c5", "2. Nc3"]);
        o!("B30", "Sicilian, Old Sicilian", ["1. e4", "c5", "2. Nf3", "Nc6"]);
        o!("B32", "Sicilian, Open/Lasker-Pelikan", ["1. e4", "c5", "2. Nf3", "Nc6", "3. d4", "cxd4", "4. Nxd4", "Nf6"]);
        o!("B40", "Sicilian Defense, French Variation", ["1. e4", "c5", "2. Nf3", "e6"]);
        o!("B50", "Sicilian, Canal Basis", ["1. e4", "c5", "2. Nf3", "d6"]);
        o!("B60", "Sicilian, Richter-Rauzer", ["1. e4", "c5", "2. Nf3", "d6", "3. d4", "cxd4", "4. Nxd4", "Nf6", "5. Nc3", "Nc6", "6. Bg5"]);
        o!("B70", "Sicilian Dragon", ["1. e4", "c5", "2. Nf3", "d6", "3. d4", "cxd4", "4. Nxd4", "Nf6", "5. Nc3", "g6"]);
        o!("B80", "Sicilian, Najdorf/Sozin setup", ["1. e4", "c5", "2. Nf3", "d6", "3. d4", "cxd4", "4. Nxd4", "Nf6", "5. Nc3", "a6"]);
        o!("B85", "Sicilian Scheveningen", ["1. e4", "c5", "2. Nf3", "d6", "3. d4", "cxd4", "4. Nxd4", "Nf6", "5. Nc3", "e6"]);
        o!("C00", "French Defense", ["1. e4", "e6"]);
        o!("C02", "French, Advance Variation", ["1. e4", "e6", "2. d4", "d5", "3. e5"]);
        o!("C03", "French, Tarrasch Variation", ["1. e4", "e6", "2. d4", "d5", "3. Nd2"]);
        o!("C10", "French, Paulsen Variation of Winawer", ["1. e4", "e6", "2. d4", "d5", "3. Nc3"]);
        o!("C20", "King's Pawn Game", ["1. e4", "e5"]);
        o!("C21", "Center Game", ["1. e4", "e5", "2. d4", "exd4", "3. Qxd4"]);
        o!("C22", "Nordic Gambit", ["1. e4", "e5", "2. d4", "exd4", "3. c3"]);
        o!("C23", "Bishop's Opening", ["1. e4", "e5", "2. Bc4"]);
        o!("C25", "Vienna Game", ["1. e4", "e5", "2. Nc3"]);
        o!("C26", "Vienna Gambit", ["1. e4", "e5", "2. Nc3", "Nf6", "3. f4"]);
        o!("C29", "King's Gambit Accepted, Classical", ["1. e4", "e5", "2. f4", "exf4", "3. Nf3", "Nf6"]);
        o!("C30", "King's Gambit Declined", ["1. e4", "e5", "2. f4"]);
        o!("C33", "King's Gambit Accepted", ["1. e4", "e5", "2. f4", "exf4"]);
        o!("C34", "King's Gambit Accepted, Cunningham-style", ["1. e4", "e5", "2. f4", "exf4", "3. Nf3", "d6"]);
        o!("C37", "King's Gambit Accepted, Muzio Gambit", ["1. e4", "e5", "2. f4", "exf4", "3. Nf3", "g5", "4. Bc4", "g4", "5. O-O"]);
        o!("C40", "Latvian Elephant", ["1. e4", "e5", "2. Nf3", "f5"]);
        o!("C41", "Philidor Defense", ["1. e4", "e5", "2. Nf3", "d6"]);
        o!("C42", "Petrov's Defense", ["1. e4", "e5", "2. Nf3", "Nf6"]);
        o!("C44", "King's Knight Opening", ["1. e4", "e5", "2. Nf3", "Nc6"]);
        o!("C45", "Scotch Game", ["1. e4", "e5", "2. Nf3", "Nc6", "3. d4"]);
        o!("C46", "Three Knights Game", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Nc3"]);
        o!("C47", "Four Knights Game", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Nc3", "Nf6"]);
        o!("C48", "Four Knights, Spanish variation", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "Nf6"]);
        o!("C50", "Italian Game", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4"]);
        o!("C51", "Evans Gambit", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Bc5", "4. b4"]);
        o!("C52", "Evans Gambit Accepted", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Bc5", "4. b4", "Bxb4"]);
        o!("C53", "Giuoco Piano", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Bc5", "4. c3"]);
        o!("C55", "Two Knights Defense", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Nf6"]);
        o!("C56", "Max Lange Attack", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Nf6", "4. d4", "exd4", "5. O-O", "Nxe4"]);
        o!("C57", "Fried Liver/Damiano defence", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Nf6", "4. Ng5"]);
        o!("C58", "Two Knights, Modern Main Line", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bc4", "Nf6", "4. Ng5", "d5", "5. exd5"]);
        o!("C60", "Ruy Lopez", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5"]);
        o!("C63", "Ruy Lopez, Schliemann/Jaenisch Gambit", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "f5"]);
        o!("C64", "Ruy Lopez, Classical Defence", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "Bc5"]);
        o!("C65", "Ruy Lopez, Berlin Defence", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "Nf6"]);
        o!("C66", "Ruy Lopez, Closed, Common variations", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Ba4", "Nf6", "5. O-O", "Be7"]);
        o!("C68", "Ruy Lopez, Exchange Variation", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Bxc6"]);
        o!("C77", "Ruy Lopez, Morphy Defence", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Ba4", "Nf6"]);
        o!("C80", "Ruy Lopez, Open Variations", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Ba4", "Nf6", "5. O-O", "Nxe4"]);
        o!("C88", "Ruy Lopez, Closed", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Ba4", "Nf6", "5. O-O", "Be7", "6. Re1", "b5"]);
        o!("C90", "Ruy Lopez, Closed, Chigorin direction", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Ba4", "Nf6", "5. O-O", "Be7", "6. Re1", "b5", "7. Bb3", "d6"]);
        o!("C95", "Ruy Lopez, Closed, Breyer", ["1. e4", "e5", "2. Nf3", "Nc6", "3. Bb5", "a6", "4. Ba4", "Nf6", "5. O-O", "Be7", "6. Re1", "b5", "7. Bb3", "d6", "8. c3", "O-O", "9. h3", "Nb8"]);
        o!("D00", "Queen's Pawn Game", ["1. d4", "d5"]);
        o!("D02", "London System", ["1. d4", "Nf6", "2. Nf3", "d5", "3. Bf4"]);
        o!("D05", "Colle System", ["1. d4", "Nf6", "2. Nf3", "d5", "3. e3"]);
        o!("D06", "Blackmar-Diemer Gambit", ["1. d4", "d5", "2. e4"]);
        o!("D10", "Slav Defense", ["1. d4", "d5", "2. c4", "c6"]);
        o!("D11", "Slav, Czech-ish lines", ["1. d4", "d5", "2. c4", "c6", "3. Nf3"]);
        o!("D15", "Slav Defense, Main Lines", ["1. d4", "d5", "2. c4", "c6", "3. Nf3", "Nf6", "4. Nc3"]);
        o!("D20", "Queen's Gambit Accepted", ["1. d4", "d5", "2. c4", "dxc4"]);
        o!("D30", "Queen's Gambit Declined", ["1. d4", "d5", "2. c4", "e6"]);
        o!("D35", "QGD, Exchange Variation", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6", "4. cxd5", "exd5"]);
        o!("D37", "QGD, Vienna/Capablanca lines", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6"]);
        o!("D43", "Semi-Tarrasch/Meran territory", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6", "4. Nf3", "c6"]);
        o!("D44", "Semi-Tarrasch Accepted", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6", "4. Nf3", "c5", "5. Bg5"]);
        o!("D50", "Queen's Gambit Declined, Tartakower idea", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6", "4. Bg5"]);
        o!("D55", "QGD, Orthodox/Queens Gambit main", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6", "4. Bg5", "Be7", "5. e3", "O-O"]);
        o!("D60", "QGD, Orthodox Defense", ["1. d4", "d5", "2. c4", "e6", "3. Nc3", "Nf6", "4. Bg5", "Be7", "5. e3", "Nbd7"]);
        o!("D70", "Grunfeld Defense", ["1. d4", "Nf6", "2. c4", "g6", "3. f3", "d5"]);
        o!("D80", "Grünfeld Defense", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "d5"]);
        o!("D85", "Grünfeld, Exchange Variation", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "d5", "4. cxd5", "Nxd5"]);
        o!("D90", "King's Indian, Gligoric/Taimanov territory", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "Bg7", "4. e4", "d6", "5. Nf3"]);
        o!("D94", "King's Indian Defense", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "Bg7", "4. e4", "d6"]);
        o!("E00", "Catalan Opening", ["1. d4", "Nf6", "2. c4", "e6", "3. g3"]);
        o!("E04", "Catalan, Open", ["1. d4", "Nf6", "2. c4", "e6", "3. g3", "d5", "4. Bg2", "dxc4"]);
        o!("E06", "Catalan, Closed", ["1. d4", "Nf6", "2. c4", "e6", "3. g3", "d5", "4. Bg2", "Be7"]);
        o!("E10", "Queen's Indian Defense", ["1. d4", "Nf6", "2. c4", "e6", "3. Nf3", "b6"]);
        o!("E12", "Queen's Indian, Petrosian system", ["1. d4", "Nf6", "2. c4", "e6", "3. Nf3", "b6", "4. e3", "Bb7", "5. Bd3"]);
        o!("E20", "Nimzo-Indian Defense", ["1. d4", "Nf6", "2. c4", "e6", "3. Nc3", "Bb4"]);
        o!("E32", "Nimzo-Indian, Classical", ["1. d4", "Nf6", "2. c4", "e6", "3. Nc3", "Bb4", "4. Qc2"]);
        o!("E40", "Nimzo-Indian, Rubinstein/Fischer ideas", ["1. d4", "Nf6", "2. c4", "e6", "3. Nc3", "Bb4", "4. e3"]);
        o!("E60", "King's Indian, Anti-grünfeld lines", ["1. d4", "Nf6", "2. c4", "g6", "3. g3"]);
        o!("E70", "King's Indian Defense, Modern Main", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "Bg7", "4. e4", "d6", "5. Be2"]);
        o!("E80", "King's Indian, Sämisch Variation", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "Bg7", "4. e4", "d6", "5. f3"]);
        o!("E90", "King's Indian, Classical Main", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "Bg7", "4. e4", "d6", "5. Nf3", "O-O", "6. Be2"]);
        o!("E97", "King's Indian, Mar del Plata", ["1. d4", "Nf6", "2. c4", "g6", "3. Nc3", "Bg7", "4. e4", "d6", "5. Nf3", "O-O", "6. Be2", "e5", "7. O-O", "Nc6"]);
        db
    }

    /// Load from Lichess-style `eco.py` (python dict literal) — tolerant regex-free parse.
    pub fn from_eco_py(text: &str) -> OpeningDb {
        let mut db = OpeningDb::default();
        for line in text.lines() {
            let l = line.trim();
            if !l.contains(": (") && !l.contains(":(") {
                continue;
            }
            // 'B20': ('Sicilian defense', '1. e4 c5'),
            let rest = l[l.find(": ").unwrap_or(0) + 2..].trim_start_matches('(');
            let q1 = match rest.find('\'').or_else(|| rest.find('"')) {
                Some(q) => q,
                None => continue,
            };
            let quote = rest.as_bytes()[q1] as char;
            let q2 = match rest[q1 + 1..].find(quote) {
                Some(k) => q1 + 1 + k,
                None => continue,
            };
            let name = rest[q1 + 1..q2].to_string();
            let after = &rest[q2 + 1..];
            let m1 = match after.find(quote) {
                Some(m) => m,
                None => continue,
            };
            let after2 = &after[m1 + 1..];
            let m2 = match after2.find(quote) {
                Some(m) => m,
                None => continue,
            };
            let moves_str = &after2[..m2];
            let eco = l[..l.find(':').unwrap_or(0)].trim().replace(['\'', '"', ' '], "");
            let sans: Vec<&str> = moves_str.split_whitespace().filter(|t| *t != ".").collect();
            // tokens like "1." then "e4": drop pure-number-with-dot tokens
            let cleaned: Vec<&str> = sans.iter().filter(|t| !is_move_number(t)).copied().collect();
            if !cleaned.is_empty() {
                db.insert(&eco, &name, &cleaned);
            }
        }
        db
    }

    /// Load from CSV: eco,name,"1. e4 c5" or JSON array of {eco,name,line}.
    pub fn from_csv(text: &str) -> OpeningDb {
        let mut db = OpeningDb::default();
        for line in text.lines() {
            let cells = crate::pgn::split_csv_pub(line);
            if cells.len() >= 3 {
                let sans: Vec<&str> = cells[2]
                    .split_whitespace()
                    .filter(|t| !is_move_number(t))
                    .collect();
                if !sans.is_empty() {
                    db.insert(&cells[0], &cells[1], &sans);
                }
            }
        }
        db
    }

    pub fn from_json(text: &str) -> OpeningDb {
        #[derive(Deserialize)]
        struct Row {
            eco: String,
            name: String,
            line: Vec<String>,
        }
        let mut db = OpeningDb::default();
        if let Ok(rows) = serde_json::from_str::<Vec<Row>>(text) {
            for r in rows {
                let refs: Vec<&str> = r.line.iter().map(|s| s.as_str()).collect();
                db.insert(&r.eco, &r.name, &refs);
            }
        }
        db
    }
    /// Merge another database into a clone of this one (used to layer user files).
    pub fn merge(&self, other: &OpeningDb) -> OpeningDb {
        let mut out = OpeningDb::default();
        self.collect(&Vec::new(), &mut out);
        other.collect(&Vec::new(), &mut out);
        out
    }

    fn collect(&self, prefix: &Vec<String>, out: &mut OpeningDb) {
        fn walk(node: &Node, prefix: &mut Vec<String>, out: &mut OpeningDb) {
            if let Some((eco, name)) = &node.opening {
                out.insert(eco, name, &prefix.iter().map(|s| s.as_str()).collect::<Vec<_>>());
            }
            for (san, child) in &node.children {
                prefix.push(san.clone());
                walk(child, prefix, out);
                prefix.pop();
            }
        }
        let mut p = prefix.clone();
        walk(&self.root, &mut p, out);
    }
}

fn is_move_number(t: &str) -> bool {
    let x = t.trim_end_matches('.');
    !x.is_empty() && x.chars().all(|c| c.is_ascii_digit())
}

/// Detect opening of a game (SAN list) using ply states to normalize SAN.
pub fn detect_opening(db: &OpeningDb, sans: &[String]) -> Option<Opening> {
    // normalize: replay and use canonical SAN so variants like e.p./0-0 map correctly
    let mut st = State::start();
    let mut canon = Vec::with_capacity(sans.len());
    for s in sans {
        match st.find_move_san(s) {
            Some(m) => {
                let san = st.san(m);
                canon.push(san);
                st = st.make(m);
            }
            None => break,
        }
    }
    db.lookup(&canon)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_italian_and_ruy() {
        let db = OpeningDb::embedded();
        let open = detect_opening(&db, &["e4".into(), "e5".into(), "Nf3".into(), "Nc6".into(), "Bc4".into()]).unwrap();
        assert_eq!(open.eco, "C50");
        let open = detect_opening(&db, &["e4".into(), "e5".into(), "Nf3".into(), "Nc6".into(), "Bb5".into(), "a6".into(), "Ba4".into(), "Nf6".into(), "O-O".into(), "Be7".into()]).unwrap();
        assert_eq!(open.eco, "C66");
    }

    #[test]
    fn longest_prefix() {
        let db = OpeningDb::embedded();
        // Najdorf should beat plain Sicilian
        let v: Vec<String> = ["e4", "c5", "Nf3", "d6", "d4", "cxd4", "Nxd4", "Nf6", "Nc3", "a6"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let o = db.lookup(&v).unwrap();
        assert_eq!(o.eco, "B80");
    }

    #[test]
    fn eco_py_parse() {
        let sample = "'B20': ('Sicilian defense', '1. e4 c5'),\n'C50': ('Giuoco piano', '1. e4 e5 2. Nf3 Nc6 3. Bc4'),";
        let db = OpeningDb::from_eco_py(sample);
        let v: Vec<String> = ["e4", "c5"].iter().map(|s| s.to_string()).collect();
        assert_eq!(db.lookup(&v).unwrap().eco, "B20");
    }
}
