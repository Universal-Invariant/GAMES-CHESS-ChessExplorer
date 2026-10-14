//! chess-core: modular chess visualizer backend.
//!
//! Modules (all extensible):
//!  - board    : rules engine (movegen, SAN/UCI/FEN)
//!  - pgn      : streaming import of PGN / chess.com JSON / CSV / SQLite DBs
//!  - db       : indexed game database with filter+sort over millions of games
//!  - filters  : boolean DSL (and/or/not/comparison/tag/line predicates)
//!  - openings : ECO trie + Lichess eco.py loader + branch explorer
//!  - analysis : pluggable analysis modules emitting JSON features per frame
//!  - engine   : UCI client (Stockfish/Lc0) + built-in fallback engine
//!  - ai       : OpenAI-compatible client for local or remote LLMs

pub mod ai;
pub mod analysis;
pub mod board;
pub mod db;
pub mod engine;
pub mod filters;
pub mod openings;
pub mod pgn;

// Re-export rusqlite so downstream crates (chess-server) can open SQLite game databases
// without declaring the dependency themselves.
pub use rusqlite;

/// Public helper: piece letter for a board cell ('p','n','b','r','q','k').
pub fn piece_letter(p: u8) -> String {
    use board::{piece_type, BISHOP, KNIGHT, PAWN, QUEEN, ROOK};
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
