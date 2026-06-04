//! AI summaries via LAMU's OpenAI-compat HTTP surface (spec §4.2).
//! One client; the `model` field selects local-vs-cloud routing inside LAMU.

use anyhow::{anyhow, Context, Result};
use serde_json::json;

pub struct AiClient {
    base_url: String,
    model: String,
    http: reqwest::Client,
}

impl AiClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self { base_url: base_url.into(), model: model.into(), http: reqwest::Client::new() }
    }

    /// Mirrors the old `askClaude(prompt, data)` contract: an instruction plus
    /// an array of grounding strings -> one text answer.
    pub async fn summarize(&self, prompt: &str, items: &[String]) -> Result<String> {
        let content = format!("{prompt}\n\n{}", items.join("\n\n"));
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": content }],
            "temperature": 0.3,
            "max_tokens": 1200
        });
        let resp = self.http.post(&url).json(&body).send().await
            .context("POST to LAMU failed — is `lamu serve` running?")?;
        let status = resp.status();
        let val: serde_json::Value = resp.json().await.context("LAMU returned non-JSON")?;
        if !status.is_success() {
            return Err(anyhow!("LAMU {status}: {val}"));
        }
        val["choices"][0]["message"]["content"]
            .as_str().map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no choices[0].message.content in LAMU response"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn summarize_posts_openai_shape_and_extracts_content() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "Summary text." } }]
            })))
            .mount(&server).await;

        let client = AiClient::new(server.uri(), "mimo-v2.5");
        let out = client.summarize("Summarize:", &["Paper A".into(), "Paper B".into()])
            .await.unwrap();
        assert_eq!(out, "Summary text.");
    }

    #[tokio::test]
    async fn summarize_errors_on_non_2xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({"error": "boom"})))
            .mount(&server).await;
        let client = AiClient::new(server.uri(), "mimo-v2.5");
        assert!(client.summarize("x", &[]).await.is_err());
    }
}
