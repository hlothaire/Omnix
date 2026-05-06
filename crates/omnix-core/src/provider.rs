use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use futures::future::BoxFuture;
use futures::{Stream, StreamExt};
use omnix_protocol::{
    ChatMessage, ContentBlock, Role, StopReason, TokenUsage, ToolChoice, ToolDefinition,
};
use reqwest::Client;
use serde::Deserialize;

pub trait Provider: Send + Sync {
    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>>;

    fn health_check(&self) -> BoxFuture<'_, Result<()>>;
}

pub struct ChatRequest {
    pub model: String,
    pub system_prompt: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: ToolChoice,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    MessageStart {
        model: String,
        usage: TokenUsage,
    },
    ContentBlockDelta {
        index: usize,
        delta: ContentBlockDelta,
    },
    ToolCallMeta {
        index: usize,
        id: String,
        name: String,
    },
    MessageDelta {
        stop_reason: Option<StopReason>,
        usage: Option<TokenUsage>,
    },
    MessageStop,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone)]
pub enum ContentBlockDelta {
    TextDelta { text: String },
    InputJsonDelta { partial_json: String },
}

pub struct LlamaCppProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl LlamaCppProvider {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
            model: model.into(),
        }
    }
}

impl Provider for LlamaCppProvider {
    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let body = self.build_request_body(request);
        let client = self.client.clone();
        let model = self.model.clone();

        Box::pin(async move {
            let body = body?;

            let response = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("Failed to connect to llama.cpp server at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("llama.cpp returned {}: {}", status, text);
            }

            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

            tokio::spawn(async move {
                let _ = tx.send(StreamEvent::MessageStart {
                    model: model.clone(),
                    usage: TokenUsage::zero(),
                });

                let mut stream = response.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(bytes) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));

                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer.drain(..=pos).collect::<String>();
                                let line = line.trim();

                                if line.is_empty() || line.starts_with(':') {
                                    continue;
                                }

                                if line == "data: [DONE]" {
                                    let _ = tx.send(StreamEvent::MessageStop);
                                    return;
                                }

                                if let Some(json_str) = line.strip_prefix("data: ") {
                                    match serde_json::from_str::<StreamChunk>(json_str) {
                                        Ok(chunk) => {
                                            if let Some(choice) = chunk.choices.first() {
                                                if let Some(delta) = &choice.delta {
                                                    if let Some(text) = &delta.content {
                                                        let _ = tx.send(
                                                            StreamEvent::ContentBlockDelta {
                                                                index: 0,
                                                                delta:
                                                                    ContentBlockDelta::TextDelta {
                                                                        text: text.clone(),
                                                                    },
                                                            },
                                                        );
                                                    }

                                                    for tc in &delta.tool_calls {
                                                        let idx = tc.index as usize;

                                                        if let (Some(id), Some(name)) =
                                                            (&tc.id, &tc.function.name)
                                                        {
                                                            let _ = tx.send(
                                                                StreamEvent::ToolCallMeta {
                                                                    index: idx,
                                                                    id: id.clone(),
                                                                    name: name.clone(),
                                                                },
                                                            );
                                                        }

                                                        if let Some(args) = &tc.function.arguments {
                                                            let _ = tx.send(
                                                                StreamEvent::ContentBlockDelta {
                                                                    index: idx,
                                                                    delta: ContentBlockDelta::InputJsonDelta {
                                                                        partial_json: args.clone(),
                                                                    },
                                                                },
                                                            );
                                                        }
                                                    }
                                                }

                                                if let Some(reason) = &choice.finish_reason {
                                                    let stop_reason = match reason.as_str() {
                                                        "stop" => Some(StopReason::EndTurn),
                                                        "tool_calls" => Some(StopReason::ToolUse),
                                                        "length" => Some(StopReason::MaxTokens),
                                                        _ => None,
                                                    };
                                                    let _ = tx.send(StreamEvent::MessageDelta {
                                                        stop_reason,
                                                        usage: None,
                                                    });
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            let _ = tx.send(StreamEvent::Error {
                                                message: format!("JSON parse error: {}", e),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(StreamEvent::Error {
                                message: format!("Stream error: {}", e),
                            });
                        }
                    }
                }

                let _ = tx.send(StreamEvent::MessageStop);
            });

            Ok(
                Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
                    as Pin<Box<dyn Stream<Item = StreamEvent> + Send>>,
            )
        })
    }

    fn health_check(&self) -> BoxFuture<'_, Result<()>> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        let model = self.model.clone();
        let client = self.client.clone();

        Box::pin(async move {
            let body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "hi"}],
                "max_tokens": 1
            });

            let response = client
                .post(&url)
                .json(&body)
                .send()
                .await
                .with_context(|| format!("Cannot connect to llama.cpp server at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("llama.cpp returned {}: {}", status, text);
            }

            Ok(())
        })
    }
}

impl LlamaCppProvider {
    fn build_request_body(&self, request: ChatRequest) -> Result<serde_json::Value> {
        let mut messages = Vec::new();

        messages.push(serde_json::json!({
            "role": "system",
            "content": request.system_prompt
        }));

        for msg in request.messages {
            match msg.role {
                Role::User => {
                    let text: String = msg
                        .content
                        .into_iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text),
                            _ => None,
                        })
                        .collect();
                    messages.push(serde_json::json!({"role": "user", "content": text}));
                }
                Role::Assistant => {
                    let mut content = String::new();
                    let mut tool_calls = Vec::new();

                    for block in msg.content {
                        match block {
                            ContentBlock::Text { text } => content.push_str(&text),
                            ContentBlock::ToolUse { id, name, input } => {
                                tool_calls.push(serde_json::json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": input.to_string()
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }

                    let mut assistant_msg = serde_json::json!({
                        "role": "assistant",
                        "content": content
                    });
                    if !tool_calls.is_empty() {
                        assistant_msg["tool_calls"] = serde_json::Value::Array(tool_calls);
                    }
                    messages.push(assistant_msg);
                }
                Role::Tool => {
                    for block in msg.content {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            ..
                        } = block
                        {
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "tool_call_id": tool_use_id,
                                "content": content
                            }));
                        }
                    }
                }
            }
        }

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true
        });

        if !request.tools.is_empty() {
            let tools: Vec<serde_json::Value> = request
                .tools
                .into_iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(tools);
        }

        match request.tool_choice {
            ToolChoice::Auto => {}
            ToolChoice::Any => {
                body["tool_choice"] = "required".into();
            }
            ToolChoice::Named(name) => {
                body["tool_choice"] = serde_json::json!({
                    "type": "function",
                    "function": { "name": name }
                });
            }
        }

        if let Some(max) = request.max_tokens {
            body["max_tokens"] = max.into();
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = temp.into();
        }

        Ok(body)
    }
}

pub struct MockProvider {
    turns: Vec<Vec<StreamEvent>>,
    call_index: AtomicUsize,
}

impl MockProvider {
    pub fn new(turns: Vec<Vec<StreamEvent>>) -> Self {
        Self {
            turns,
            call_index: AtomicUsize::new(0),
        }
    }
}

impl Provider for MockProvider {
    fn stream_chat(
        &self,
        _request: ChatRequest,
    ) -> BoxFuture<'_, Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>> {
        let idx = self.call_index.fetch_add(1, Ordering::SeqCst);
        let events = self.turns.get(idx).cloned().unwrap_or_default();

        Box::pin(async move {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            for event in events {
                let _ = tx.send(event);
            }
            let _ = tx.send(StreamEvent::MessageStop);
            Ok(
                Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
                    as Pin<Box<dyn Stream<Item = StreamEvent> + Send>>,
            )
        })
    }

    fn health_check(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: Option<StreamDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<StreamToolCallDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCallDelta {
    index: i32,
    #[serde(default)]
    id: Option<String>,
    function: StreamFunctionDelta,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}
