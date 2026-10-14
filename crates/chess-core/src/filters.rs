//! Filter DSL over indexed games. Grammar:
//!   expr := or
//!   or   := and ( 'or' | '||' | ',' )? and ...
//!   and  := not ( 'and' | '&&' )? not ...
//!   not  := 'not'/'!' not | '(' expr ')' | pred
//!   pred := field ':' value | field '>=' num | field '<=' num | field '>' num | field '<' num
//!          | 'opening' ~ text | 'line' '="e4 e5"' | tag-predicate [Key="Value"]
//! Fields: white black eco result date elo_w elo_b moves min_elo max_elo num_moves opening player
//! Values are matched case-insensitively; strings support substring match when prefixed '~'.

use crate::db::IndexedGame;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Pred {
    Eq { field: String, value: String },
    Sub { field: String, value: String },
    NumCmp { field: String, op: CmpOp, value: f64 },
    Line(String), // exact SAN line, e.g. "1.e4 e5 2.Nf3"
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Gt,
    Lt,
    Ge,
    Le,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Expr {
    True,
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Leaf(Pred),
}

#[derive(Clone, Debug)]
struct Tok {
    kind: TokKind,
    text: String,
}

#[derive(Clone, Debug, PartialEq)]
enum TokKind {
    Ident,
    Num,
    Str,
    Op, // : > < >= <= = ~ && || ! , ( ) [ ]
}

fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let b = src.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '"' || c == '\'' {
            let q = c;
            let start = i + 1;
            i += 1;
            while i < b.len() && b[i] as char != q {
                i += 1;
            }
            out.push(Tok {
                kind: TokKind::Str,
                text: src[start..i.min(src.len())].to_string(),
            });
            i += 1;
            continue;
        }
        for (op, len) in [(">=", 2), ("<=", 2), ("&&", 2), ("||", 2)] {
            if src[i..].starts_with(op) {
                out.push(Tok {
                    kind: TokKind::Op,
                    text: op.to_string(),
                });
                i += len;
                break;
            }
        }
        if i < b.len() && matches!(b[i] as char, ':' | '>' | '<' | '=' | '~' | '!' | ',' | '(' | ')' | '[' | ']') {
            out.push(Tok {
                kind: TokKind::Op,
                text: (b[i] as char).to_string(),
            });
            i += 1;
            continue;
        }
        if i >= b.len() {
            break;
        }
        let start = i;
        while i < b.len() {
            let ch = b[i] as char;
            if ch.is_alphanumeric() || "_-./".contains(ch) {
                i += 1;
            } else {
                break;
            }
        }
        if start == i {
            return Err(format!("unexpected char {:?}", b[i] as char));
        }
        let text = &src[start..i];
        let kind = if text.bytes().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && text.parse::<f64>().is_ok() {
            TokKind::Num
        } else {
            TokKind::Ident
        };
        out.push(Tok { kind, text: text.to_string() });
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn eat_op(&mut self, ops: &[&str]) -> Option<String> {
        let hit = self.peek().map(|t| t.kind == TokKind::Op && ops.contains(&t.text.as_str())).unwrap_or(false);
        if hit {
            let text = self.toks[self.pos].text.clone();
            self.pos += 1;
            return Some(text);
        }
        None
    }
    fn eat_kw(&mut self, kw: &str) -> bool {
        let hit = self.peek().map(|t| t.kind == TokKind::Ident && t.text.eq_ignore_ascii_case(kw)).unwrap_or(false);
        if hit {
            self.pos += 1;
            return true;
        }
        false
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_and()?;
        loop {
            if self.eat_op(&["||"]).is_some() || self.eat_kw("or") {
                let right = self.parse_and()?;
                left = Expr::Or(Box::new(left), Box::new(right));
            } else if self.eat_op(&[","]).is_some() {
                // comma at top level = OR (common filter UI convention)
                let right = self.parse_and()?;
                left = Expr::Or(Box::new(left), Box::new(right));
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_not()?;
        loop {
            if self.eat_op(&["&&"]).is_some() || self.eat_kw("and") {
                let right = self.parse_not()?;
                left = Expr::And(Box::new(left), Box::new(right));
            } else {
                // juxtaposition: next token starts a predicate → implicit AND
                match self.peek() {
                    Some(t) if t.kind == TokKind::Ident || t.text == "(" || t.text == "[" => {
                        let right = self.parse_not()?;
                        left = Expr::And(Box::new(left), Box::new(right));
                    }
                    _ => break,
                }
            }
        }
        Ok(left)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if self.eat_op(&["!"]).is_some() || self.eat_kw("not") {
            return Ok(Expr::Not(Box::new(self.parse_not()?)));
        }
        if self.eat_op(&["("]).is_some() {
            let e = self.parse_expr()?;
            if self.eat_op(&[")"]).is_none() {
                return Err("expected )".into());
            }
            return Ok(e);
        }
        if self.eat_op(&["["]).is_some() {
            // [Key="Value"] PGN-tag predicate
            let key = self.expect_ident()?;
            if self.eat_op(&["="]).is_none() {
                return Err("expected = in tag predicate".into());
            }
            let val = match self.peek() {
                Some(t) if t.kind == TokKind::Str || t.kind == TokKind::Ident => {
                    let s = t.text.clone();
                    self.pos += 1;
                    s
                }
                _ => return Err("expected tag value".into()),
            };
            if self.eat_op(&["]"]).is_none() {
                return Err("expected ]".into());
            }
            return Ok(Expr::Leaf(Pred::Sub {
                field: format!("tag:{}", key.to_ascii_lowercase()),
                value: val,
            }));
        }
        // predicate: field op value
        let field = self.expect_ident()?;
        let op = match self.peek() {
            Some(t) if t.kind == TokKind::Op => t.text.clone(),
            _ => return Err(format!("expected operator after field '{}'", field)),
        };
        self.pos += 1;
        match op.as_str() {
            ":" | "~" | "=" => {
                let valtok = self.peek().cloned().ok_or("expected value")?;
                self.pos += 1;
                let field = field.to_ascii_lowercase();
                if field == "line" {
                    return Ok(Expr::Leaf(Pred::Line(valtok.text)));
                }
                if op == "~" {
                    return Ok(Expr::Leaf(Pred::Sub { field, value: valtok.text }));
                }
                // ':' means equality for enums / substring for names — decide by field
                Ok(Expr::Leaf(match field.as_str() {
                    "white" | "black" | "player" | "opening" | "event" | "site" => Pred::Sub {
                        field,
                        value: valtok.text,
                    },
                    _ => Pred::Eq {
                        field,
                        value: valtok.text,
                    },
                }))
            }
            ">" | "<" | ">=" | "<=" => {
                let numtok = self.peek().cloned().ok_or("expected number")?;
                self.pos += 1;
                let v: f64 = numtok.text.replace('_', "").parse().map_err(|_| "bad number")?;
                let cmp = match op.as_str() {
                    ">" => CmpOp::Gt,
                    "<" => CmpOp::Lt,
                    ">=" => CmpOp::Ge,
                    _ => CmpOp::Le,
                };
                Ok(Expr::Leaf(Pred::NumCmp {
                    field: field.to_ascii_lowercase(),
                    op: cmp,
                    value: v,
                }))
            }
            other => Err(format!("unknown operator {}", other)),
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek() {
            Some(t) if t.kind == TokKind::Ident || t.kind == TokKind::Str => {
                let s = t.text.clone();
                self.pos += 1;
                Ok(s)
            }
            _ => Err("expected identifier".into()),
        }
    }
}

pub fn parse_filter(src: &str) -> Result<Expr, String> {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return Ok(Expr::True);
    }
    let toks = tokenize(trimmed)?;
    let mut p = Parser { toks, pos: 0 };
    let e = p.parse_expr()?;
    if p.pos != p.toks.len() {
        return Err(format!("trailing tokens near {:?}", p.peek().map(|t| &t.text)));
    }
    Ok(e)
}

fn field_num(g: &IndexedGame, f: &str) -> Option<f64> {
    match f {
        "elo_w" | "white_elo" | "whiteelo" => Some(g.white_elo as f64),
        "elo_b" | "black_elo" | "blackelo" => Some(g.black_elo as f64),
        "min_elo" => Some(g.white_elo.min(g.black_elo) as f64),
        "max_elo" => Some(g.white_elo.max(g.black_elo) as f64),
        "moves" | "num_moves" | "ply_len" => Some(g.num_moves as f64),
        "year" => g.utc_date.get(0..4).and_then(|y| y.parse::<f64>().ok()),
        "score" => Some(g.score),
        _ => None,
    }
}

fn field_str<'a>(g: &'a IndexedGame, f: &str) -> Option<String> {
    match f {
        "white" => Some(g.white.clone()),
        "black" => Some(g.black.clone()),
        "player" => Some(format!("{} {}", g.white, g.black)),
        "eco" => Some(g.eco.clone()),
        "opening" => Some(g.opening.clone()),
        "result" => Some(g.result.clone()),
        "date" | "utc_date" => Some(g.utc_date.clone()),
        "termination" => Some(g.termination.clone()),
        _ => None,
    }
}

pub fn eval(expr: &Expr, g: &IndexedGame) -> bool {
    match expr {
        Expr::True => true,
        Expr::And(a, b) => eval(a, g) && eval(b, g),
        Expr::Or(a, b) => eval(a, g) || eval(b, g),
        Expr::Not(a) => !eval(a, g),
        Expr::Leaf(p) => eval_pred(p, g),
    }
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    hay.to_lowercase().contains(&needle.to_lowercase())
}

fn eval_pred(p: &Pred, g: &IndexedGame) -> bool {
    match p {
        Pred::Sub { field, value } => {
            if let Some(f) = field.strip_prefix("tag:") {
                return g.tags.iter().any(|(k, v)| k.eq_ignore_ascii_case(f) && contains_ci(v, value));
            }
            match field_str(g, field) {
                Some(s) => contains_ci(&s, value),
                None => false,
            }
        }
        Pred::Eq { field, value } => {
            if field == "eco" {
                return g.eco.eq_ignore_ascii_case(value);
            }
            if field == "result" {
                return g.result == *value;
            }
            if let Some(n) = field_num(g, field) {
                if let Ok(v) = value.parse::<f64>() {
                    return n == v;
                }
            }
            match field_str(g, field) {
                Some(s) => s.eq_ignore_ascii_case(value),
                None => false,
            }
        }
        Pred::NumCmp { field, op, value } => {
            if let Some(n) = field_num(g, field) {
                match op {
                    CmpOp::Gt => n > *value,
                    CmpOp::Lt => n < *value,
                    CmpOp::Ge => n >= *value,
                    CmpOp::Le => n <= *value,
                }
            } else {
                false
            }
        }
        Pred::Line(line) => {
            let want = normalize_line(line);
            let have = normalize_line(&g.first_moves);
            have.starts_with(&want)
        }
    }
}

pub fn normalize_line(s: &str) -> String {
    s.split_whitespace()
        .filter(|t| {
            let x = t.trim_end_matches('.');
            !(x.chars().all(|c| c.is_ascii_digit()) && !x.is_empty())
        })
        .map(|t| t.trim_end_matches('.').to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::IndexedGame;

    fn game() -> IndexedGame {
        IndexedGame {
            id: 1,
            white: "Carlsen".into(),
            black: "Hikaru".into(),
            white_elo: 2850,
            black_elo: 2780,
            utc_date: "2023.04.01".into(),
            result: "1-0".into(),
            eco: "B30".into(),
            opening: "Sicilian Defense".into(),
            termination: "".into(),
            num_moves: 41,
            score: 1.0,
            first_moves: "e4 c5 Nf3 Nc6".into(),
            tags: vec![("Event".into(), "Champions Chess Tour".into())],
        }
    }

    #[test]
    fn simple_and_compound() {
        let g = game();
        let e = parse_filter("eco:B30").unwrap();
        assert!(eval(&e, &g));
        let e = parse_filter("white:carlsen and elo_w>2800").unwrap();
        assert!(eval(&e, &g));
        let e = parse_filter("(result:1-0 or result:0-1) and opening~sicilian").unwrap();
        assert!(eval(&e, &g));
        let e = parse_filter("not black:hikaru").unwrap();
        assert!(!eval(&e, &g));
        let e = parse_filter("[Event=\"Champions\"]").unwrap();
        assert!(eval(&e, &g));
        let e = parse_filter("line:\"e4 c5\"").unwrap();
        assert!(eval(&e, &g));
        let e = parse_filter("min_elo>=2700, moves<50").unwrap();
        assert!(eval(&e, &g));
    }
}
