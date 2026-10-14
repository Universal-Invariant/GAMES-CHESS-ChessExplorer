//! Indexed game database (SQLite) built from streamed imports.
//! Stores summaries + compact move lists so millions of games stay fast to
//! filter/sort; full PGN text is kept for playback.

use crate::board::State;
use crate::filters::{self, Expr};
use crate::openings::{detect_opening, OpeningDb};
use crate::pgn::Game;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndexedGame {
    pub id: i64,
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
    pub first_moves: String,
    pub tags: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameRow {
    pub id: i64,
    pub pgn: String,
}

pub struct GameDb {
    pub conn: Connection,
}

impl GameDb {
    pub fn open(path: &str) -> rusqlite::Result<GameDb> {
        let conn = if path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(path)?
        };
        conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS games(
              id INTEGER PRIMARY KEY,
              white TEXT, black TEXT,
              white_elo INTEGER, black_elo INTEGER,
              utc_date TEXT, result TEXT,
              eco TEXT, opening TEXT, termination TEXT,
              num_moves INTEGER, score REAL,
              first_moves TEXT, tags TEXT,
              pgn TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_eco ON games(eco);
            CREATE INDEX IF NOT EXISTS idx_white ON games(white);
            CREATE INDEX IF NOT EXISTS idx_black ON games(black);
            CREATE INDEX IF NOT EXISTS idx_elo ON games(white_elo, black_elo);
            CREATE TABLE IF NOT EXISTS analyses(
              game_id INTEGER, ply INTEGER,
              cp INTEGER, depth INTEGER, best TEXT, pv TEXT,
              eval_before INTEGER, class TEXT, ai_note TEXT,
              PRIMARY KEY(game_id, ply)
            );
            CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v TEXT);
            "#,
        )?;
        Ok(GameDb { conn })
    }

    pub fn count(&self) -> i64 {
        self.conn
            .query_row("SELECT COUNT(*) FROM games", [], |r| r.get(0))
            .unwrap_or(0)
    }

    pub fn add_game(&mut self, game: &Game, odb: &OpeningDb) -> rusqlite::Result<i64> {
        let s = game.summary();
        let plies = game.replay();
        // canonical SAN line from replay (falls back to raw tokens)
        let sans: Vec<String> = if !plies.is_empty() && plies.len() == game.moves.len() {
            plies.iter().map(|p| p.san.clone()).collect()
        } else {
            game.moves.clone()
        };
        let opening = detect_opening(odb, &sans);
        let (eco, name) = match &opening {
            Some(o) => (o.eco.clone(), o.name.clone()),
            None => (s.eco.clone(), s.opening.clone()),
        };
        let first_moves: String = sans.iter().take(16).cloned().collect::<Vec<_>>().join(" ");
        let tags_json = serde_json::to_string(&game.tags).unwrap_or_else(|_| "[]".into());
        let pgn = render_pgn(game, &eco, &name, &sans);
        self.conn.execute(
            "INSERT INTO games(white,black,white_elo,black_elo,utc_date,result,eco,opening,termination,num_moves,score,first_moves,tags,pgn)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![s.white, s.black, s.white_elo, s.black_elo, s.utc_date, s.result, eco, name, s.termination,
                    s.num_moves as i64, s.score, first_moves, tags_json, pgn],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Filter + sort + paginate using SQL prefilter then DSL evaluation.
    pub fn query(&self, expr: &Expr, sort: &str, desc: bool, offset: i64, limit: i64) -> Vec<IndexedGame> {
        // Push down simple predicates into SQL for speed on huge tables.
        let mut sql = String::from("SELECT id,white,black,white_elo,black_elo,utc_date,result,eco,opening,termination,num_moves,score,first_moves,tags FROM games");
        let mut conds: Vec<String> = Vec::new();
        let mut bind: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        pushdown(expr, &mut conds, &mut bind);
        if !conds.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conds.join(" AND "));
        }
        let allowed_sort = [
            ("white", "white"),
            ("black", "black"),
            ("elo_w", "white_elo"),
            ("elo_b", "black_elo"),
            ("min_elo", "MIN(white_elo,black_elo)"),
            ("max_elo", "MAX(white_elo,black_elo)"),
            ("date", "utc_date"),
            ("eco", "eco"),
            ("opening", "opening"),
            ("moves", "num_moves"),
            ("result", "result"),
        ];
        let col = allowed_sort
            .iter()
            .find(|(k, _)| *k == sort)
            .map(|(_, c)| *c)
            .unwrap_or("id");
        sql.push_str(&format!(" ORDER BY {} {}", col, if desc { "DESC" } else { "ASC" }));
        // when no pushdown happened we must over-fetch and post-filter
        let has_pushdown = !conds.is_empty();
        let fetch_limit = if has_pushdown { limit } else { limit * 4 + 100 };
        sql.push_str(&format!(" LIMIT {} OFFSET {}", fetch_limit.max(1), offset));
        let refs: Vec<&dyn rusqlite::types::ToSql> = bind.iter().map(|b| b.as_ref()).collect();
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(refs.as_slice(), row_to_indexed);
        let mut out = Vec::new();
        if let Ok(rows) = rows {
            for r in rows.flatten() {
                if has_pushdown || filters::eval(expr, &r) {
                    out.push(r);
                    if out.len() as i64 >= limit {
                        break;
                    }
                }
            }
        }
        out
    }

    pub fn get_pgn(&self, id: i64) -> Option<String> {
        self.conn
            .query_row("SELECT pgn FROM games WHERE id=?1", params![id], |r| r.get(0))
            .ok()
    }

    pub fn get_game(&self, id: i64) -> Option<Game> {
        let pgn = self.get_pgn(id)?;
        crate::pgn::parse_pgn(&pgn)
    }

    pub fn save_analysis(&mut self, game_id: i64, ply: usize, cp: i32, depth: u32, best: &str, pv: &str, class: &str, ai_note: &str) {
        let _ = self.conn.execute(
            "INSERT OR REPLACE INTO analyses(game_id,ply,cp,depth,best,pv,class,ai_note) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![game_id, ply as i64, cp, depth as i64, best, pv, class, ai_note],
        );
    }

    pub fn load_analyses(&self, game_id: i64) -> Vec<(usize, i32, u32, String, String, String)> {
        let mut stmt = match self.conn.prepare(
            "SELECT ply,cp,depth,best,pv,class FROM analyses WHERE game_id=?1 ORDER BY ply",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map(params![game_id], |r| {
            Ok((
                r.get::<_, i64>(0)? as usize,
                r.get::<_, i32>(1)?,
                r.get::<_, i64>(2)? as u32,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
            ))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Distinct opening histogram for explorer-style display.
    pub fn opening_stats(&self) -> Vec<(String, String, i64)> {
        let mut stmt = match self.conn.prepare(
            "SELECT eco, opening, COUNT(*) c FROM games WHERE eco!='' GROUP BY eco, opening ORDER BY c DESC LIMIT 100",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }
}

fn row_to_indexed(r: &rusqlite::Row) -> rusqlite::Result<IndexedGame> {
    let tags_json: String = r.get(13)?;
    let tags: Vec<(String, String)> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(IndexedGame {
        id: r.get(0)?,
        white: r.get(1)?,
        black: r.get(2)?,
        white_elo: r.get(3)?,
        black_elo: r.get(4)?,
        utc_date: r.get(5)?,
        result: r.get(6)?,
        eco: r.get(7)?,
        opening: r.get(8)?,
        termination: r.get(9)?,
        num_moves: r.get::<_, i64>(10)? as usize,
        score: r.get(11)?,
        first_moves: r.get(12)?,
        tags,
    })
}

/// Best-effort SQL pushdown for common predicate shapes.
fn pushdown(e: &Expr, conds: &mut Vec<String>, bind: &mut Vec<Box<dyn rusqlite::types::ToSql>>) {
    match e {
        Expr::And(a, b) => {
            pushdown(a, conds, bind);
            pushdown(b, conds, bind);
        }
        Expr::Leaf(filters::Pred::Eq { field, value }) => match field.as_str() {
            "eco" => {
                conds.push("eco LIKE ?".into());
                bind.push(Box::new(value.to_uppercase()));
            }
            "result" => {
                conds.push("result = ?".into());
                bind.push(Box::new(value.clone()));
            }
            _ => {}
        },
        Expr::Leaf(filters::Pred::Sub { field, value }) => {
            let col: Option<(&str, String)> = match field.as_str() {
                "white" => Some(("white", format!("%{}%", value))),
                "black" => Some(("black", format!("%{}%", value))),
                "player" => None,
                "opening" => Some(("opening", format!("%{}%", value))),
                _ => None,
            };
            if let Some((c, v)) = col {
                conds.push(format!("{} LIKE ?", c));
                bind.push(Box::new(v));
            }
        }
        Expr::Leaf(filters::Pred::NumCmp { field, op, value }) => {
            let col: Option<(&str, f64)> = match field.as_str() {
                "elo_w" | "white_elo" => Some(("white_elo", *value)),
                "elo_b" | "black_elo" => Some(("black_elo", *value)),
                "min_elo" => Some(("MIN(white_elo,black_elo)", *value)),
                "max_elo" => Some(("MAX(white_elo,black_elo)", *value)),
                "moves" | "num_moves" => Some(("num_moves", *value)),
                _ => None,
            };
            if let Some((c, v)) = col {
                let o = match op {
                    filters::CmpOp::Gt => ">",
                    filters::CmpOp::Lt => "<",
                    filters::CmpOp::Ge => ">=",
                    filters::CmpOp::Le => "<=",
                };
                conds.push(format!("{} {}", c, o));
                bind.push(Box::new(v));
            }
        }
        Expr::Leaf(filters::Pred::Line(line)) => {
            conds.push("first_moves LIKE ?".into());
            bind.push(Box::new(format!("{}%", filters::normalize_line(line))));
        }
        _ => {}
    }
}

pub fn render_pgn(game: &Game, eco: &str, opening: &str, sans: &[String]) -> String {
    let mut out = String::new();
    for (k, v) in &game.tags {
        if k == "FEN" || k == "StartFEN" {
            continue;
        }
        out.push_str(&format!("[{} \"{}\"]\n", k, v));
    }
    if !eco.is_empty() {
        out.push_str(&format!("[ECO \"{}\"]\n", eco));
    }
    if !opening.is_empty() {
        out.push_str(&format!("[Opening \"{}\"]\n", opening));
    }
    if let Some(fen) = &game.start_fen {
        out.push_str(&format!("[FEN \"{}\"]\n[SetUp \"1\"]\n", fen));
    }
    let mut mt = String::new();
    let mut st = game
        .start_fen
        .as_deref()
        .and_then(State::from_fen)
        .unwrap_or_else(State::start);
    let moveno = st.fullmove;
    let mut line_len = 0;
    for (i, san) in sans.iter().enumerate() {
        // move number prefix on white's moves (or first ply of a black-to-move start FEN)
        let is_start_of_white_move = i == 0 && st.side == 1;
        let token = if st.side == 0 || is_start_of_white_move {
            format!("{}. {}", st.fullmove, san)
        } else {
            san.clone()
        };
        if line_len + token.len() + 1 > 80 {
            out.push_str(mt.trim_end());
            out.push('\n');
            mt.clear();
            line_len = 0;
        }
        mt.push_str(&token);
        mt.push(' ');
        line_len += token.len() + 1;
        if let Some(m) = st.find_move_san(san) {
            st = st.make(m);
        }
    }
    let _ = moveno;
    out.push_str(mt.trim_end());
    out.push_str(&format!("\n{}", game.tag("Result")));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pgn::parse_pgn;

    #[test]
    fn index_query_filter() {
        let mut db = GameDb::open(":memory:").unwrap();
        let odb = OpeningDb::embedded();
        let pgn = "[White \"Anand\"]\n[Black \"Topalov\"]\n[WhiteElo \"2783\"]\n[BlackElo \"2772\"]\n[Result \"1-0\"]\n[UTCDate \"2013.03.15\"]\n\n1. d4 Nf6 2. c4 g6 3. Nc3 Bg7 4. e4 d6 5. Be2 O-O 6. Be3 e5 7. d5 Na6 8. Bg5 1-0";
        let g = parse_pgn(pgn).unwrap();
        let id = db.add_game(&g, &odb).unwrap();
        assert!(id >= 1);
        let e = filters::parse_filter("eco:D90 or eco:E70").unwrap();
        let res = db.query(&e, "date", false, 0, 10);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].white, "Anand");
        let e2 = filters::parse_filter("min_elo>2780").unwrap();
        assert_eq!(db.query(&e2, "date", false, 0, 10).len(), 1);
        let e3 = filters::parse_filter("min_elo>2783").unwrap();
        assert_eq!(db.query(&e3, "date", false, 0, 10).len(), 0);
    }
}
