//! chess-viz: HTTP API + static frontend host + CLI.
//!
//! Endpoints:
//!   POST /api/import        {path}                     stream-import any supported file into the DB
//!   GET  /api/games?filter=&sort=&desc=&offset=&limit= filter/sort/paginate millions of games
//!   GET  /api/game?id=                                 full game (tags, SAN, plies)
//!   GET  /api/frames?id=&engine=&ms=                   per-ply analysis frames (modules + engine evals + classes)
//!   POST /api/ai            {question?, analyze?, fen?, context?}
//!   GET  /api/openings?prefix=e4 c5                    opening explorer branches (+ db stats)
//!   POST /api/openings/load {format:"eco_py|csv|json", text} extend the opening trie at runtime
//!   GET  /api/config                                   AI/engine configuration
//!   POST /api/config                                   change AI base_url/model or engine binary live
//!   GET  /api/status                                   counts, engine name, features list

mod http;

use chess_core::{
    ai::{AiClient, AiConfig},
    analysis::{self, classify_move, default_modules, EvalPoint},
    board::State,
    db::GameDb,
    engine::{get_engine, ChessEngine},
    filters,
    openings::OpeningDb,
    pgn::{parse_pgn, PgnCursor},
};
use http::{HttpServer, Request, Response};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

struct AppState {
    db: GameDb,
    openings: OpeningDb,
    ai_cfg: AiConfig,
    engine_bin: Option<String>,
    modules: Vec<Box<dyn analysis::AnalysisModule>>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut port = 8123u16;
    let mut db_path = "data/games.db".to_string();
    let mut web_root = "web".to_string();
    let mut cli_import: Option<String> = None;
    let mut cli_filter: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--port" => {
                port = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(port);
                i += 1;
            }
            "--db" => {
                db_path = args[i + 1].clone();
                i += 1;
            }
            "--web" => {
                web_root = args[i + 1].clone();
                i += 1;
            }
            "--import" => {
                cli_import = Some(args[i + 1].clone());
                i += 1;
            }
            "--filter" => {
                cli_filter = Some(args[i + 1].clone());
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let mut state = AppState {
        db: GameDb::open(&db_path).expect("open db"),
        openings: load_openings(),
        ai_cfg: AiConfig::default(),
        engine_bin: std::env::var("CHESS_ENGINE").ok(),
        modules: default_modules(),
    };

    if let Some(path) = cli_import.clone() {
        let n = do_import(&mut state, &path);
        println!("imported {} games (total {})", n, state.db.count());
        if let Some(f) = &cli_filter {
            let expr = filters::parse_filter(f).unwrap_or_else(|e| panic!("bad filter: {}", e));
            for g in state.db.query(&expr, "date", false, 0, 25) {
                println!("#{} {} vs {} [{} {}] {} {} ({})", g.id, g.white, g.black, g.eco, g.opening, g.result, g.utc_date, g.num_moves);
            }
        }
        return;
    }

    let state = Arc::new(Mutex::new(state));

    let mut server = HttpServer::new();

    server.route(
        "POST",
        "/api/import",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let body = http::json_body(req).unwrap_or(json!({}));
                let path = body["path"].as_str().unwrap_or("");
                if path.is_empty() {
                    return Response::err("missing path");
                }
                let mut s = st.lock().unwrap();
                let n = do_import(&mut s, path);
                Response::json(json!({ "imported": n, "total": s.db.count() }).to_string())
            }
        }),
    );

    server.route(
        "GET",
        "/api/games",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let f = req.query.get("filter").cloned().unwrap_or_default();
                let sort = req.query.get("sort").cloned().unwrap_or_else(|| "date".into());
                let desc = req.query.get("desc").map(|s| s == "1" || s == "true").unwrap_or(false);
                let offset: i64 = req.query.get("offset").and_then(|s| s.parse().ok()).unwrap_or(0);
                let limit: i64 = req.query.get("limit").and_then(|s| s.parse().ok()).unwrap_or(50);
                let expr = match filters::parse_filter(&f) {
                    Ok(e) => e,
                    Err(e) => return Response::err(&format!("filter parse error: {}", e)),
                };
                let s = st.lock().unwrap();
                let games = s.db.query(&expr, &sort, desc, offset, limit);
                Response::json(json!({ "games": games, "total_in_db": s.db.count() }).to_string())
            }
        }),
    );

    server.route(
        "GET",
        "/api/game",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let id: i64 = match req.query.get("id").and_then(|s| s.parse().ok()) {
                    Some(i) => i,
                    None => return Response::err("missing id"),
                };
                let s = st.lock().unwrap();
                match s.db.get_game(id) {
                    Some(g) => {
                        let plies = g.replay();
                        let sum = g.summary();
                        Response::json(
                            json!({
                                "id": id,
                                "tags": g.tags,
                                "moves": g.moves,
                                "plies": plies,
                                "summary": sum,
                                "pgn": s.db.get_pgn(id),
                            })
                            .to_string(),
                        )
                    }
                    None => Response::err("game not found"),
                }
            }
        }),
    );

    server.route(
        "GET",
        "/api/frames",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let id: i64 = match req.query.get("id").and_then(|s| s.parse().ok()) {
                    Some(i) => i,
                    None => return Response::err("missing id"),
                };
                let ms: u64 = req.query.get("ms").and_then(|s| s.parse().ok()).unwrap_or(120);
                let use_engine = req.query.get("engine").map(|s| s != "0").unwrap_or(true);
                let mut s = st.lock().unwrap();
                let game = match s.db.get_game(id) {
                    Some(g) => g,
                    None => return Response::err("game not found"),
                };
                let sans: Vec<String> = game.replay().into_iter().map(|p| p.san).collect();
                // module features per ply
                let mod_frames = analysis::analyze_game(&s.modules, &sans, game.start_fen.as_deref());
                // engine evaluations + classification
                let mut engine_evals: Vec<(usize, i32, String, String)> = Vec::new();
                let mut evals_by_ply: Vec<Option<(i32, String)>> = Vec::new();
                if use_engine {
                    let mut eng = get_engine(s.engine_bin.as_deref());
                    let mut stpos = game
                        .start_fen
                        .as_deref()
                        .and_then(State::from_fen)
                        .unwrap_or_else(State::start);
                    let prev_evals = s.db.load_analyses(id);
                    let mut cache: std::collections::HashMap<usize, (i32, String)> = prev_evals
                        .into_iter()
                        .map(|(p, cp, _, best, _, _)| (p, (cp, best)))
                        .collect();
                    for (i, san) in sans.iter().enumerate() {
                        let fen = stpos.to_fen();
                        let ev = if let Some(v) = cache.get(&i) {
                            v.clone()
                        } else {
                            match eng.evaluate(&fen, ms) {
                                Ok(e) => (e.cp, e.best),
                                Err(_) => (0, String::new()),
                            }
                        };
                        cache.insert(i, ev.clone());
                        evals_by_ply.push(Some(ev));
                        if let Some(m) = stpos.find_move_san(san) {
                            stpos = stpos.make(m);
                        }
                    }
                    // final position eval to classify last move
                    let fin = eng.evaluate(&stpos.to_fen(), ms).map(|e| e.cp).unwrap_or(0);
                    let mut prev = evals_by_ply.first().cloned().flatten();
                    let _ = fin;
                    for (i, san) in sans.iter().enumerate() {
                        let before = evals_by_ply[i];
                        let after = if i + 1 < evals_by_ply.len() {
                            evals_by_ply[i + 1]
                        } else {
                            Some((fin, String::new()))
                        };
                        let mover_white = i % 2 == 0;
                        let class = match (before, after) {
                            (Some(b), Some(a)) => classify_move(
                                EvalPoint { cp: b.0, mate: None },
                                EvalPoint { cp: a.0, mate: None },
                                mover_white,
                            )
                            .to_string(),
                            _ => "good".to_string(),
                        };
                        engine_evals.push((i, before.map(|b| b.0).unwrap_or(0), before.map(|b| b.1.clone()).unwrap_or_default(), class));
                        prev = before;
                    }
                    // persist analyses
                    for (ply, cp, best, class) in &engine_evals {
                        s.db.save_analysis(id, *ply, *cp, 0, best, "", class, "");
                    }
                }
                let out: Vec<Value> = mod_frames
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let feats: Vec<(String, Value)> = f.features.clone();
                        let (cp, best, class) = engine_evals
                            .get(i)
                            .map(|e| (e.1, e.2.clone(), e.3.clone()))
                            .unwrap_or((0, String::new(), "good".into()));
                        json!({
                            "ply": f.ply,
                            "fen": f.fen,
                            "san": f.san,
                            "uci": f.uci,
                            "eval_cp": cp,
                            "best_uci": best,
                            "class": class,
                            "features": feats,
                        })
                    })
                    .collect();
                Response::json(json!({ "frames": out, "engine": if use_engine { "auto" } else { "off" } }).to_string())
            }
        }),
    );

    server.route(
        "POST",
        "/api/ai",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let body = match http::json_body(req) {
                    Ok(b) => b,
                    Err(e) => return Response::err(&e),
                };
                let mut s = st.lock().unwrap();
                let mut client = AiClient::new(s.ai_cfg.clone());
                if let Some(game_id) = body["game_id"].as_i64() {
                    if body["analyze"].as_bool().unwrap_or(false) {
                        let game = match s.db.get_game(game_id) {
                            Some(g) => g,
                            None => return Response::err("game not found"),
                        };
                        let sans: Vec<String> = game.replay().into_iter().map(|p| p.san).collect();
                        let evals = s
                            .db
                            .load_analyses(game_id)
                            .into_iter()
                            .map(|(p, cp, _, best, _, class)| (p, cp, best, class))
                            .collect::<Vec<_>>();
                        return match client.analyze_game(&game.tags, &sans, &evals) {
                            Ok(text) => Response::json(json!({ "analysis": text }).to_string()),
                            Err(e) => Response::err(&e),
                        };
                    }
                }
                let question = body["question"].as_str().unwrap_or("");
                if question.is_empty() {
                    return Response::err("missing question");
                }
                let fen = body["fen"].as_str().unwrap_or("startpos");
                let context = body["context"].as_str().unwrap_or("");
                match client.question_about_position(fen, question, context) {
                    Ok(text) => Response::json(json!({ "answer": text }).to_string()),
                    Err(e) => Response::err(&e),
                }
            }
        }),
    );

    server.route(
        "GET",
        "/api/openings",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let prefix = req.query.get("prefix").cloned().unwrap_or_default();
                let s = st.lock().unwrap();
                let sans: Vec<String> = prefix
                    .split_whitespace()
                    .filter(|t| !t.chars().all(|c| c.is_ascii_digit() || c == '.'))
                    .map(|t| t.to_string())
                    .collect();
                let current = if sans.is_empty() {
                    None
                } else {
                    s.openings.lookup(&sans).map(|o| json!({"eco": o.eco, "name": o.name}))
                };
                let children = s
                    .openings
                    .children_of(&sans)
                    .into_iter()
                    .map(|(san, open)| json!({"san": san, "eco": open.as_ref().map(|o| o.0.clone()), "name": open.as_ref().map(|o| o.1.clone())}))
                    .collect::<Vec<_>>();
                let stats = s
                    .db
                    .opening_stats()
                    .into_iter()
                    .map(|(eco, name, c)| json!({"eco": eco, "name": name, "games": c}))
                    .collect::<Vec<_>>();
                Response::json(json!({"current": current, "branches": children, "db_openings": stats, "opening_entries": s.openings.len()}).to_string())
            }
        }),
    );

    server.route(
        "POST",
        "/api/openings/load",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let body = match http::json_body(req) {
                    Ok(b) => b,
                    Err(e) => return Response::err(&e),
                };
                let fmt = body["format"].as_str().unwrap_or("eco_py");
                let text = body["text"].as_str().unwrap_or("");
                let mut s = st.lock().unwrap();
                let odb = match fmt {
                    "csv" => OpeningDb::from_csv(text),
                    "json" => OpeningDb::from_json(text),
                    _ => OpeningDb::from_eco_py(text),
                };
                let added = odb.len();
                // layer the loaded openings on top of the current trie
                let merged = s.openings.merge(&odb);
                s.openings = merged;
                Response::json(json!({ "added": added, "total": s.openings.len() }).to_string())
            }
        }),
    );

    server.route(
        "GET",
        "/api/config",
        Box::new({
            let st = state.clone();
            move |_req: &Request| {
                let s = st.lock().unwrap();
                Response::json(
                    json!({
                        "ai": s.ai_cfg,
                        "engine_binary": s.engine_bin,
                        "modules": s.modules.iter().map(|m| m.name()).collect::<Vec<_>>(),
                    })
                    .to_string(),
                )
            }
        }),
    );

    server.route(
        "POST",
        "/api/config",
        Box::new({
            let st = state.clone();
            move |req: &Request| {
                let body = match http::json_body(req) {
                    Ok(b) => b,
                    Err(e) => return Response::err(&e),
                };
                let mut s = st.lock().unwrap();
                if let Some(ai) = body.get("ai") {
                    if let Some(u) = ai["base_url"].as_str() {
                        s.ai_cfg.base_url = u.to_string();
                    }
                    if let Some(k) = ai["api_key"].as_str() {
                        s.ai_cfg.api_key = k.to_string();
                    }
                    if let Some(m) = ai["model"].as_str() {
                        s.ai_cfg.model = m.to_string();
                    }
                    if let Some(t) = ai["temperature"].as_f64() {
                        s.ai_cfg.temperature = t as f32;
                    }
                }
                if let Some(eb) = body["engine_binary"].as_str() {
                    s.engine_bin = if eb.is_empty() { None } else { Some(eb.to_string()) };
                }
                Response::json(json!({ "ok": true, "ai": s.ai_cfg, "engine_binary": s.engine_bin }).to_string())
            }
        }),
    );

    server.route(
        "GET",
        "/api/status",
        Box::new({
            let st = state.clone();
            move |_req: &Request| {
                let s = st.lock().unwrap();
                let mut eng = get_engine(None);
                Response::json(
                    json!({
                        "games": s.db.count(),
                        "openings": s.openings.len(),
                        "engine_available": eng.name(),
                        "modules": s.modules.iter().map(|m| m.name()).collect::<Vec<_>>(),
                    })
                    .to_string(),
                )
            }
        }),
    );

    server.serve(&format!("0.0.0.0:{}", port), Some(web_root));
}

fn load_openings() -> OpeningDb {
    // optional user file data/openings.eco.py (Lichess format) layered on embedded set
    let db = OpeningDb::embedded();
    if let Ok(text) = std::fs::read_to_string("data/openings.eco.py") {
        let extra = OpeningDb::from_eco_py(&text);
        return db.merge(&extra);
    }
    db
}

fn do_import(state: &mut AppState, path: &str) -> i64 {
    let mut n = 0i64;
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".db") || lower.ends_with(".sqlite") || lower.ends_with(".sqlite3") {
        if let Ok(conn) = rusqlite_conn(path) {
            if let Ok(games) = chess_core::pgn::import_sqlite_games(path, &conn) {
                for g in games {
                    let _ = state.db.add_game(&g, &state.openings);
                    n += 1;
                }
            }
        }
        return n;
    }
    if let Ok(cursor) = PgnCursor::open(path) {
        let mut cursor = cursor;
        while let Some(game) = cursor.next_game() {
            if !game.moves.is_empty() {
                let _ = state.db.add_game(&game, &state.openings);
                n += 1;
            }
        }
    }
    n
}

fn rusqlite_conn(path: &str) -> Result<rusqlite::Connection, String> {
    rusqlite::Connection::open(path).map_err(|e| e.to_string())
}

// small helper used by tests/CLI elsewhere
pub fn parse_one_pgn(text: &str) -> Option<chess_core::pgn::Game> {
    parse_pgn(text)
}
