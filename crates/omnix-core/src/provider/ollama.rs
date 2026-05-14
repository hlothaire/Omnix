use std::collections::HashMap;
use std::pin::Pin;

use anyhow::{Context, Result};
use futures::Stream;
use futures::future::BoxFuture;
use reqwest::Client;
use serde::Deserialize;

use super::{
    ChatRequest, Provider, StreamEvent, build_request_body, parse_sse_stream, retry_request,
};

#[derive(Clone)]
pub struct OllamaProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            base_url: base_url.into(),
            model: model.into(),
        }
    }
}

impl Provider for OllamaProvider {
    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };
        let body = build_request_body(&model, request);
        let client = self.client.clone();

        Box::pin(async move {
            let body = body?;

            let response = retry_request(|| client.post(&url).json(&body).send(), 2)
                .await
                .with_context(|| format!("Failed to connect to Ollama at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                let hint = if status.as_u16() == 404 {
                    " (Hint: Model not found. Run 'ollama pull <model>' first.)"
                } else if status.as_u16() == 500 && text.contains("model") {
                    " (Hint: Model not loaded. Try running 'ollama run <model>' first.)"
                } else if status.as_u16() == 503 {
                    " (Hint: Ollama is still loading the model. Wait a moment and retry.)"
                } else {
                    ""
                };
                anyhow::bail!("Ollama returned {}: {}{}", status, text, hint);
            }

            Ok(parse_sse_stream(response, model))
        })
    }

    fn health_check(&self) -> BoxFuture<'_, Result<()>> {
        let url = format!("{}/api/tags", self.base_url);
        let client = self.client.clone();

        Box::pin(async move {
            let response = retry_request(|| client.get(&url).send(), 2)
                .await
                .with_context(|| format!("Cannot connect to Ollama at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Ollama returned {}: {}", status, text);
            }

            Ok(())
        })
    }

    fn context_window(&self) -> BoxFuture<'_, Result<usize>> {
        let url = format!("{}/api/show", self.base_url);
        let model = self.model.clone();
        let client = self.client.clone();

        Box::pin(async move {
            let body = serde_json::json!({ "model": model });

            let response = retry_request(|| client.post(&url).json(&body).send(), 2)
                .await
                .with_context(|| format!("Cannot query Ollama model info at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Ollama returned {}: {}", status, text);
            }

            let payload: OllamaShowResponse = response
                .json()
                .await
                .with_context(|| "Failed to parse /api/show response")?;

            payload
                .context_length()
                .map(|n| n as usize)
                .with_context(|| "Ollama /api/show response did not include a context length")
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        let url = format!("{}/api/tags", self.base_url);
        let client = self.client.clone();

        Box::pin(async move {
            let response = retry_request(|| client.get(&url).send(), 2)
                .await
                .with_context(|| format!("Cannot query Ollama models at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Ollama returned {}: {}", status, text);
            }

            let payload: OllamaTagsResponse = response
                .json()
                .await
                .with_context(|| "Failed to parse /api/tags response")?;

            Ok(payload.models.into_iter().map(|m| m.name).collect())
        })
    }

    fn summarize(&self, text: String, max_tokens: u32) -> BoxFuture<'_, Result<String>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let model = self.model.clone();
        let client = self.client.clone();

        Box::pin(async move {
            let body = serde_json::json!({
                "model": model,
                "messages": [
                    {"role": "system", "content": "You are a summarization assistant. Create a concise, structured summary of the conversation provided by the user. Preserve key facts, decisions, file paths, and next steps."},
                    {"role": "user", "content": text}
                ],
                "max_tokens": max_tokens,
                "temperature": 0.3,
                "stream": false
            });

            let response = retry_request(|| client.post(&url).json(&body).send(), 2)
                .await
                .with_context(|| format!("Failed to summarize with Ollama at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("Ollama returned {}: {}", status, text);
            }

            let payload: serde_json::Value = response
                .json()
                .await
                .with_context(|| "Failed to parse summarization response")?;

            let summary = payload["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();

            Ok(summary)
        })
    }
}

#[derive(Debug, Deserialize, Default)]
struct OllamaShowResponse {
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    model_info: HashMap<String, serde_json::Value>,
}

impl OllamaShowResponse {
    fn context_length(&self) -> Option<u64> {
        self.context_length.or_else(|| {
            self.model_info.iter().find_map(|(key, value)| {
                if key == "context_length" || key.ends_with(".context_length") {
                    value.as_u64()
                } else {
                    None
                }
            })
        })
    }
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelInfo>,
}

#[derive(Debug, Deserialize)]
struct OllamaModelInfo {
    name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ollama_show_response_parsing() {
        let json = r#"{"context_length": 65536}"#;
        let resp: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.context_length(), Some(65536));
    }

    #[test]
    fn test_ollama_show_model_info_context_parsing() {
        let json = r#"{"model_info": {"llama.context_length": 131072}}"#;
        let resp: OllamaShowResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.context_length(), Some(131072));
    }

    #[test]
    fn test_ollama_tags_response_parsing() {
        let json = r#"{"models": [{"name": "test-model:latest"}]}"#;
        let resp: OllamaTagsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.models.len(), 1);
        assert_eq!(resp.models[0].name, "test-model:latest");
    }
}
