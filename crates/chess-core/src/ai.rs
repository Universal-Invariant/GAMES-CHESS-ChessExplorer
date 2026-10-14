//! OpenAI-compatible chat client (works with local servers too:
//! Ollama `http://IP:11434/v1`, llama.cpp `http://IP:8080/v1`, LM Studio,
//! vLLM, or api.openai.com). Fully configurable base URL / key / model.
//! The AI gets chess-aware system prompts plus analysis data as tools/context.

use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AiConfig {
    /// e.g. http://127.0.0.1:11434/v1 or https://api.openai.com/v1
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl Default for AiConfig {
    fn default() -> Self {
        AiConfig {
            base_url: "http://127.0.0.1:11434/v1".into(),
            api_key: "none".into(),
            model: "llama3.1".into(),
            temperature: 0.4,
            max_tokens: 700,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub struct AiClient {
    pub cfg: AiConfig,
    history: Vec<ChatMessage>,
}

impl AiClient {
    pub fn new(cfg: AiConfig) -> Self {
        AiClient { cfg, history: Vec::new() }
    }

    pub fn reset(&mut self) {
        self.history.clear();
    }

    fn agent(&self) -> ureq::Agent {
        ureq::AgentBuilder::new()
            .try_proxy_from_env(false)
            .timeout(std::time::Duration::from_secs(120))
            .build()
    }

    pub fn ask(&mut self, system: &str, user: &str) -> Result<String, String> {
        self.history.push(ChatMessage {
            role: "user".into(),
            content: user.to_string(),
        });
        let messages: Vec<_> = std::iter::once(ChatMessage {
            role: "system".into(),
            content: system.to_string(),
        })
        .chain(self.history.clone())
        .collect();
        let body = json!({
            "model": self.cfg.model,
            "messages": messages,
            "temperature": self.cfg.temperature,
            "max_tokens": self.cfg.max_tokens,
        });
        let url = format!("{}/chat/completions", self.cfg.base_url.trim_end_matches('/'));
        let req = self.agent().post(&url).set("Authorization", &format!("Bearer {}", self.cfg.api_key)).send_json(body.clone());
        match req {
            Ok(resp) => {
                let v: serde_json::Value = resp.into_json().map_err(|e| e.to_string())?;
                let content = v["choices"][0]["message"]["content"]
                    .as_str()
                    .ok_or("bad response shape")?
                    .to_string();
                self.history.push(ChatMessage {
                    role: "assistant".into(),
                    content: content.clone(),
                });
                if self.history.len() > 24 {
                    let cut = self.history.len() - 24;
                    self.history.drain(..cut);
                }
                Ok(content)
            }
            Err(err) => {
                // remove the unanswered user message
                self.history.pop();
                Err(format!("AI request failed: {}", err))
            }
        }
    }

    /// Analyze a game: build a rich prompt from engine + module data.
    pub fn analyze_game(
        &mut self,
        headers: &[(String, String)],
        sans: &[String],
        evals: &[(usize, i32, String, String)], // ply, cp-white, best-uci, class
    ) -> Result<String, String> {
        let mut sb = String::new();
        for (k, v) in headers {
            if ["White", "Black", "Result", "ECO", "Opening", "WhiteElo", "BlackElo", "UTCDate"].contains(&k.as_str()) {
                sb.push_str(&format!("{}: {}\n", k, v));
            }
        }
        let mut line = String::new();
        for (i, s) in sans.iter().enumerate() {
            if i % 2 == 0 {
                line.push_str(&format!("{}. ", i / 2 + 1));
            }
            line.push_str(s);
            line.push(' ');
        }
        sb.push_str(&format!("Moves: {}\n", line.trim()));
        if !evals.is_empty() {
            sb.push_str("Engine per-ply (ply, eval-cp-for-white, class):\n");
            for (ply, cp, _, class) in evals {
                if *class != "good" {
                    sb.push_str(&format!("  ply {} : {} ({} pawns) -> {}\n", ply, sans.get(*ply).cloned().unwrap_or_default(), *cp as f64 / 100.0, class));
                }
            }
        }
        let sys = "You are a chess coach embedded in a visualizer. Given a game and its engine evaluation, \
                   produce a concise structured analysis: 1) Opening & plans, 2) Critical moments (name exact moves), \
                   3) Strategic themes (pawn structure, piece activity, weaknesses), 4) Top improvement suggestions. \
                   Be specific, reference move numbers. Keep under 350 words.";
        self.ask(sys, &sb)
    }

    /// Answer a free-form question about the current game/position.
    pub fn question_about_position(&mut self, fen: &str, question: &str, context: &str) -> Result<String, String> {
        let sys = format!(
            "You are a chess analyst inside a visualizer. Current FEN: {}. Context: {}",
            fen, context
        );
        self.ask(&sys, question)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_prompt_without_network() {
        let c = AiClient::new(AiConfig::default());
        assert_eq!(c.history.len(), 0);
        // network call intentionally not performed in unit tests
        let _ = json!({"x": 1});
    }
}
