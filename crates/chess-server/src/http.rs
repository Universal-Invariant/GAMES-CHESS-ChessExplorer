//! Minimal blocking HTTP/1.1 server (thread-per-connection) exposing the
//! chess-core modules as a JSON API for the web frontend and CLI usage.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub body: String,
}

pub struct Response {
    pub status: u16,
    pub body: String,
    pub content_type: String,
}

impl Response {
    pub fn json(s: String) -> Response {
        Response {
            status: 200,
            body: s,
            content_type: "application/json; charset=utf-8".into(),
        }
    }
    pub fn err(msg: &str) -> Response {
        Response {
            status: 500,
            body: format!(r#"{{"error":"{}"}}"#, msg.replace('"', "'")),
            content_type: "application/json; charset=utf-8".into(),
        }
    }
    pub fn html(s: String) -> Response {
        Response {
            status: 200,
            body: s,
            content_type: "text/html; charset=utf-8".into(),
        }
    }
}

pub type Handler = Box<dyn Fn(&Request) -> Response + Send + Sync>;

pub struct HttpServer {
    routes: Vec<(String, String, Handler)>, // method, path pattern, handler
}

impl HttpServer {
    pub fn new() -> Self {
        HttpServer { routes: Vec::new() }
    }
    pub fn route(&mut self, method: &str, path: &str, h: Handler) {
        self.routes.push((method.to_string(), path.to_string(), h));
    }

    pub fn serve(self, addr: &str, root: Option<String>) {
        let listener = TcpListener::bind(addr).expect("bind");
        eprintln!("chess-viz server listening on http://{}", addr);
        let shared_root: Arc<Option<String>> = Arc::new(root);
        let routes = Arc::new(self.routes);
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let routes = routes.clone();
                let root = shared_root.clone();
                std::thread::spawn(move || {
                    let _ = handle_conn(stream, &routes, root.as_ref());
                });
            }
        }
    }
}

fn parse_req(mut stream: TcpStream) -> Option<Request> {
    let mut buf = vec![0u8; 65536];
    let mut total = 0usize;
    // read headers
    loop {
        let n = stream.read(&mut buf[total..]).ok()?;
        if n == 0 {
            break;
        }
        total += n;
        if buf[..total].windows(4).any(|w| w == b"\r\n\r\n") || buf[..total].windows(2).any(|w| w == b"\n\n") {
            break;
        }
        if total == buf.len() {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf[..total]).to_string();
    let header_end = text.find("\r\n\r\n").map(|i| i + 4).or_else(|| text.find("\n\n").map(|i| i + 2))?;
    let head = &text[..header_end];
    let mut lines = head.lines();
    let first = lines.next()?.split_whitespace().collect::<Vec<_>>();
    if first.len() < 2 {
        return None;
    }
    let method = first[0].to_string();
    let full_path = first[1].to_string();
    let (path, qs) = match full_path.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (full_path.clone(), String::new()),
    };
    let mut query = HashMap::new();
    for kv in qs.split('&') {
        if let Some((k, v)) = kv.split_once('=') {
            query.insert(urldecode(k), urldecode(v));
        }
    }
    let mut body = text[header_end..].to_string();
    // if Content-Length says more than we have, keep reading
    if let Some(cl) = head.lines().find_map(|l| {
        let l = l.to_ascii_lowercase();
        l.strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0))
    }) {
        while body.len() < cl {
            let mut extra = [0u8; 8192];
            match stream.read(&mut extra) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.push_str(&String::from_utf8_lossy(&extra[..n])),
            }
        }
        body.truncate(cl);
    }
    Some(Request {
        method,
        path,
        query,
        body,
    })
}

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn handle_conn(mut stream: TcpStream, routes: &Arc<Vec<(String, String, Handler)>>, root: &Option<String>) -> std::io::Result<()> {
    let req = match parse_req(stream.try_clone()?) {
        Some(r) => r,
        None => return Ok(()),
    };
    let resp = dispatch(&req, routes, root);
    let body_bytes = resp.body.as_bytes();
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        resp.status,
        status_text(resp.status),
        resp.content_type,
        body_bytes.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body_bytes)?;
    stream.flush()
}

fn status_text(s: u16) -> &'static str {
    match s {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    }
}

fn dispatch(req: &Request, routes: &Arc<Vec<(String, String, Handler)>>, root: &Option<String>) -> Response {
    for (m, p, h) in routes.iter() {
        if m == &req.method && p == &req.path {
            return h(req);
        }
    }
    // static files
    if req.method == "GET" {
        if let Some(dir) = root.as_deref() {
            let rel = req.path.trim_start_matches('/');
            let rel = if rel.is_empty() { "index.html" } else { rel };
            let mut full = std::path::PathBuf::from(dir);
            full.push(rel);
            // prevent traversal
            if full.starts_with(dir) {
                if let Ok(data) = std::fs::read(&full) {
                    let ct = match full.extension().and_then(|e| e.to_str()).unwrap_or("") {
                        "html" => "text/html; charset=utf-8",
                        "js" => "text/javascript; charset=utf-8",
                        "css" => "text/css; charset=utf-8",
                        "json" => "application/json",
                        "png" => "image/png",
                        "svg" => "image/svg+xml",
                        _ => "application/octet-stream",
                    };
                    return Response {
                        status: 200,
                        body: String::from_utf8_lossy(&data).to_string(),
                        content_type: ct.into(),
                    };
                }
            }
        }
    }
    Response {
        status: 404,
        body: r#"{"error":"not found"}"#.into(),
        content_type: "application/json".into(),
    }
}

pub fn json_body(req: &Request) -> Result<serde_json::Value, String> {
    serde_json::from_str(&req.body).map_err(|e| e.to_string())
}


