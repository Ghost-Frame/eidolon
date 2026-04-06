use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grammar: Option<String>,
    pub stream: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionResponse {
    pub choices: Vec<Choice>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Choice {
    pub message: ResponseMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ResponseMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Streaming delta from SSE
#[derive(Debug, Clone, Deserialize)]
pub struct StreamChunk {
    pub choices: Vec<StreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamChoice {
    pub delta: StreamDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StreamDelta {
    pub content: Option<String>,
}

pub struct LlmClient {
    base_url: String,
    http: reqwest::Client,
}

impl LlmClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Check if an error is transient and worth retrying.
    fn is_transient(status: reqwest::StatusCode) -> bool {
        matches!(status.as_u16(), 429 | 502 | 503 | 504)
    }

    /// Non-streaming completion. Used for structured output (routing, tool calls).
    pub async fn complete(&self, request: &ChatCompletionRequest) -> Result<ChatCompletionResponse, reqwest::Error> {
        let mut req = request.clone();
        req.stream = false;

        let mut retries = 0u32;
        loop {
            let resp = self.http
                .post(format!("{}/v1/chat/completions", self.base_url))
                .json(&req)
                .send()
                .await?;

            if resp.status().is_success() || !Self::is_transient(resp.status()) || retries >= 3 {
                return resp.json().await;
            }

            retries += 1;
            let delay = std::time::Duration::from_millis(500 * (1 << retries.min(3)));
            tokio::time::sleep(delay).await;
        }
    }

    /// Streaming completion. Sends text deltas through the channel.
    /// Returns the full accumulated response text when done.
    pub async fn stream_complete(
        &self,
        request: &ChatCompletionRequest,
        tx: mpsc::UnboundedSender<String>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let mut req = request.clone();
        req.stream = true;

        let mut retries = 0u32;
        let response = loop {
            match self.http
                .post(format!("{}/v1/chat/completions", self.base_url))
                .json(&req)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() || !Self::is_transient(resp.status()) || retries >= 3 => {
                    break resp;
                }
                Ok(_) => {
                    retries += 1;
                    let delay = std::time::Duration::from_millis(500 * (1 << retries.min(3)));
                    tokio::time::sleep(delay).await;
                }
                Err(e) if retries < 3 && e.is_connect() => {
                    retries += 1;
                    let delay = std::time::Duration::from_millis(500 * (1 << retries.min(3)));
                    tokio::time::sleep(delay).await;
                }
                Err(e) => return Err(Box::new(e)),
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("HTTP {}: {}", status, body).into());
        }

        let mut accumulated = String::new();
        let mut stream = response.bytes_stream();

        use futures::StreamExt;
        let mut buffer = String::new();

        'stream: while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            let text = String::from_utf8_lossy(chunk.as_ref());
            buffer.push_str(&text);

            // Process complete SSE lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        break 'stream;
                    }
                    match serde_json::from_str::<StreamChunk>(data) {
                        Ok(chunk) => {
                            for choice in &chunk.choices {
                                if let Some(content) = &choice.delta.content {
                                    accumulated.push_str(content);
                                    let _ = tx.send(content.clone());
                                }
                            }
                        }
                        Err(_) => {
                            let _ = tx.send("[LLM parse error]".to_string());
                        }
                    }
                }
            }
        }

        Ok(accumulated)
    }

    /// Build a chat completion request from conversation messages.
    pub fn build_request(
        messages: &[(&str, &str)],
        temperature: f32,
        grammar: Option<&str>,
    ) -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: None,
            messages: messages
                .iter()
                .map(|(role, content)| ChatMessage {
                    role: role.to_string(),
                    content: content.to_string(),
                })
                .collect(),
            temperature: Some(temperature),
            max_tokens: Some(4096),
            grammar: grammar.map(|g| g.to_string()),
            stream: false,
        }
    }

    /// Build request with explicit model name.
    pub fn build_request_with_model(
        model: &str,
        messages: &[(&str, &str)],
        temperature: f32,
        grammar: Option<&str>,
    ) -> ChatCompletionRequest {
        let mut req = Self::build_request(messages, temperature, grammar);
        req.model = Some(model.to_string());
        req
    }
}
