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

pub mod ollama;

pub trait Provider: Send + Sync {
    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>>;

    fn health_check(&self) -> BoxFuture<'_, Result<()>>;

    fn context_window(&self) -> BoxFuture<'_, Result<usize>>;

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<String>>>;

    fn summarize(&self, text: String, max_tokens: u32) -> BoxFuture<'_, Result<String>>;
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

/// Retry an async operation with exponential backoff.
async fn retry_request<F, Fut>(operation: F, max_retries: u32) -> Result<reqwest::Response>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
{
    let mut last_error = None;
    for attempt in 0..=max_retries {
        match operation().await {
            Ok(response) => return Ok(response),
            Err(e) => {
                last_error = Some(e);
                if attempt < max_retries {
                    let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt));
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
    Err(anyhow::anyhow!(
        "Failed after {} retries: {}",
        max_retries,
        last_error.unwrap()
    ))
}

/// Build an OpenAI-compatible chat request body.
fn build_request_body(model: &str, request: ChatRequest) -> Result<serde_json::Value> {
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
                        model_content,
                        ..
                    } = block
                    {
                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": model_content.unwrap_or(content)
                        }));
                    }
                }
            }
        }
    }

    let mut body = serde_json::json!({
        "model": model,
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

    // Request usage stats in streaming responses (supported by llama.cpp, OpenAI, etc.)
    body["stream_options"] = serde_json::json!({"include_usage": true});

    Ok(body)
}

/// Turn an HTTP response body into a stream of `StreamEvent` by parsing SSE.
fn parse_sse_stream(
    response: reqwest::Response,
    model: String,
) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
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
                                    // Capture usage from any chunk that provides it
                                    if let Some(ref u) = chunk.usage {
                                        let _ = tx.send(StreamEvent::MessageDelta {
                                            stop_reason: None,
                                            usage: Some(TokenUsage {
                                                input_tokens: u.prompt_tokens,
                                                output_tokens: u.completion_tokens,
                                                cache_creation_tokens: 0,
                                                cache_read_tokens: 0,
                                            }),
                                        });
                                    }

                                    if let Some(choice) = chunk.choices.first() {
                                        if let Some(delta) = &choice.delta {
                                            if let Some(text) = &delta.content {
                                                let _ = tx.send(StreamEvent::ContentBlockDelta {
                                                    index: 0,
                                                    delta: ContentBlockDelta::TextDelta {
                                                        text: text.clone(),
                                                    },
                                                });
                                            }

                                            for tc in &delta.tool_calls {
                                                let idx = tc.index as usize;

                                                if let (Some(id), Some(name)) =
                                                    (&tc.id, &tc.function.name)
                                                {
                                                    let _ = tx.send(StreamEvent::ToolCallMeta {
                                                        index: idx,
                                                        id: id.clone(),
                                                        name: name.clone(),
                                                    });
                                                }

                                                if let Some(args) = &tc.function.arguments {
                                                    let _ =
                                                        tx.send(StreamEvent::ContentBlockDelta {
                                                            index: idx,
                                                            delta:
                                                                ContentBlockDelta::InputJsonDelta {
                                                                    partial_json: args.clone(),
                                                                },
                                                        });
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
                                            let usage = chunk.usage.as_ref().map(|u| TokenUsage {
                                                input_tokens: u.prompt_tokens,
                                                output_tokens: u.completion_tokens,
                                                cache_creation_tokens: 0,
                                                cache_read_tokens: 0,
                                            });
                                            let _ = tx.send(StreamEvent::MessageDelta {
                                                stop_reason,
                                                usage,
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

    Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
}

pub struct LlamaCppProvider {
    client: Client,
    base_url: String,
    model: String,
}

impl LlamaCppProvider {
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

impl Provider for LlamaCppProvider {
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
                .with_context(|| format!("Failed to connect to llama.cpp server at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                let hint = if status.as_u16() == 404 {
                    " (Hint: Model not loaded on server. Check llama.cpp is running with the correct model.)"
                } else if status.as_u16() == 503 {
                    " (Hint: Server is overloaded or model is still loading.)"
                } else if status.as_u16() == 401 {
                    " (Hint: Authentication required. Check API key if configured.)"
                } else {
                    ""
                };
                anyhow::bail!("llama.cpp returned {}: {}{}", status, text, hint);
            }

            Ok(parse_sse_stream(response, model))
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

            let response = retry_request(|| client.post(&url).json(&body).send(), 2)
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

    fn context_window(&self) -> BoxFuture<'_, Result<usize>> {
        let props_url = format!("{}/props", self.base_url);
        let client = self.client.clone();

        Box::pin(async move {
            let response = retry_request(|| client.get(&props_url).send(), 1)
                .await
                .with_context(|| format!("Cannot query llama.cpp runtime properties at {}", props_url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("llama.cpp returned {}: {}", status, text);
            }

            let props: LlamaPropsResponse = response
                .json()
                .await
                .with_context(|| "Failed to parse /props response")?;

            props
                .context_size()
                .map(|n| n as usize)
                .with_context(|| "llama.cpp /props response did not include active runtime context size")
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        let url = format!("{}/v1/models", self.base_url);
        let client = self.client.clone();

        Box::pin(async move {
            let response = retry_request(|| client.get(&url).send(), 2)
                .await
                .with_context(|| format!("Cannot query model at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("llama.cpp returned {}: {}", status, text);
            }

            let payload: LlamaModelsResponse = response
                .json()
                .await
                .with_context(|| "Failed to parse /v1/models response")?;

            Ok(payload.data.into_iter().map(|m| m.id).collect())
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
                .with_context(|| format!("Failed to summarize with llama.cpp at {}", url))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                anyhow::bail!("llama.cpp returned {}: {}", status, text);
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

#[derive(Debug, Deserialize)]
struct LlamaModelsResponse {
    data: Vec<LlamaModelInfo>,
}

#[derive(Debug, Deserialize)]
struct LlamaModelInfo {
    id: String,
}

#[derive(Debug, Deserialize, Default)]
struct LlamaPropsResponse {
    #[serde(default)]
    n_ctx: Option<u64>,
    #[serde(default)]
    context_length: Option<u64>,
    #[serde(default)]
    default_generation_settings: Option<LlamaGenerationSettings>,
}

impl LlamaPropsResponse {
    fn context_size(&self) -> Option<u64> {
        self.n_ctx
            .or(self.context_length)
            .or_else(|| {
                self.default_generation_settings
                    .as_ref()
                    .and_then(|s| s.context_size())
            })
    }
}

#[derive(Debug, Deserialize, Default)]
struct LlamaGenerationSettings {
    #[serde(default)]
    n_ctx: Option<u64>,
    #[serde(default)]
    params: Option<LlamaGenerationParams>,
}

impl LlamaGenerationSettings {
    fn context_size(&self) -> Option<u64> {
        self.n_ctx
            .or_else(|| self.params.as_ref().and_then(|p| p.n_ctx))
    }
}

#[derive(Debug, Deserialize, Default)]
struct LlamaGenerationParams {
    #[serde(default)]
    n_ctx: Option<u64>,
}

pub enum AnyProvider {
    LlamaCpp(LlamaCppProvider),
    Ollama(ollama::OllamaProvider),
    #[doc(hidden)]
    Mock(MockProvider),
}

impl AnyProvider {
    pub fn from_kind(kind: &str, host: &str, model: &str) -> Result<Self> {
        match kind {
            "ollama" => {
                let h = if host == "http://localhost:8080" || host.is_empty() {
                    "http://localhost:11434"
                } else {
                    host
                };
                Ok(AnyProvider::Ollama(ollama::OllamaProvider::new(h, model)))
            }
            "llama_cpp" | "llamacpp" => {
                let h = if host == "http://localhost:11434" || host.is_empty() {
                    "http://localhost:8080"
                } else {
                    host
                };
                Ok(AnyProvider::LlamaCpp(LlamaCppProvider::new(h, model)))
            }
            _ => anyhow::bail!("Unknown provider kind: {}", kind),
        }
    }
}

impl Provider for AnyProvider {
    fn stream_chat(
        &self,
        request: ChatRequest,
    ) -> BoxFuture<'_, Result<Pin<Box<dyn Stream<Item = StreamEvent> + Send>>>> {
        match self {
            AnyProvider::LlamaCpp(p) => p.stream_chat(request),
            AnyProvider::Ollama(p) => p.stream_chat(request),
            AnyProvider::Mock(p) => p.stream_chat(request),
        }
    }

    fn health_check(&self) -> BoxFuture<'_, Result<()>> {
        match self {
            AnyProvider::LlamaCpp(p) => p.health_check(),
            AnyProvider::Ollama(p) => p.health_check(),
            AnyProvider::Mock(p) => p.health_check(),
        }
    }

    fn context_window(&self) -> BoxFuture<'_, Result<usize>> {
        match self {
            AnyProvider::LlamaCpp(p) => p.context_window(),
            AnyProvider::Ollama(p) => p.context_window(),
            AnyProvider::Mock(p) => p.context_window(),
        }
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        match self {
            AnyProvider::LlamaCpp(p) => p.list_models(),
            AnyProvider::Ollama(p) => p.list_models(),
            AnyProvider::Mock(p) => p.list_models(),
        }
    }

    fn summarize(&self, text: String, max_tokens: u32) -> BoxFuture<'_, Result<String>> {
        match self {
            AnyProvider::LlamaCpp(p) => p.summarize(text, max_tokens),
            AnyProvider::Ollama(p) => p.summarize(text, max_tokens),
            AnyProvider::Mock(p) => p.summarize(text, max_tokens),
        }
    }
}

pub struct MockProvider {
    turns: Vec<Vec<StreamEvent>>,
    call_index: AtomicUsize,
    context_window: usize,
    context_error: Option<String>,
    summarize_error: Option<String>,
    summary_response: Option<String>,
}

impl MockProvider {
    pub fn new(turns: Vec<Vec<StreamEvent>>) -> Self {
        Self {
            turns,
            call_index: AtomicUsize::new(0),
            context_window: 4096,
            context_error: None,
            summarize_error: None,
            summary_response: None,
        }
    }

    pub fn with_context_window(mut self, context_window: usize) -> Self {
        self.context_window = context_window;
        self
    }

    pub fn with_context_error(mut self, error: impl Into<String>) -> Self {
        self.context_error = Some(error.into());
        self
    }

    pub fn with_summarize_error(mut self, error: impl Into<String>) -> Self {
        self.summarize_error = Some(error.into());
        self
    }

    pub fn with_summary_response(mut self, summary: impl Into<String>) -> Self {
        self.summary_response = Some(summary.into());
        self
    }

    pub fn stream_call_count(&self) -> usize {
        self.call_index.load(Ordering::SeqCst)
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

    fn context_window(&self) -> BoxFuture<'_, Result<usize>> {
        let context_window = self.context_window;
        let error = self.context_error.clone();
        Box::pin(async move {
            if let Some(error) = error {
                anyhow::bail!(error);
            }
            Ok(context_window)
        })
    }

    fn list_models(&self) -> BoxFuture<'_, Result<Vec<String>>> {
        Box::pin(async move { Ok(vec!["mock-model".to_string()]) })
    }

    fn summarize(&self, _text: String, _max_tokens: u32) -> BoxFuture<'_, Result<String>> {
        let error = self.summarize_error.clone();
        let summary = self.summary_response.clone();
        Box::pin(async move {
            if let Some(error) = error {
                anyhow::bail!(error);
            }
            Ok(summary.unwrap_or_else(|| {
                "Mock summary: the conversation covered various topics and tasks.".to_string()
            }))
        })
    }
}

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default)]
struct StreamUsage {
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
}

#[derive(Debug, Deserialize, Default)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<StreamUsage>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llama_models_response_parsing() {
        let json = r#"{"data": [{"id": "test.gguf", "meta": {"n_ctx_train": 32768}}]}"#;
        let resp: LlamaModelsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.data.len(), 1);
        assert_eq!(resp.data[0].id, "test.gguf");
    }

    #[test]
    fn test_llama_props_context_parsing() {
        let json = r#"{"default_generation_settings": {"n_ctx": 8192}}"#;
        let props: LlamaPropsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(props.context_size(), Some(8192));
    }

    #[test]
    fn test_llama_props_nested_params_context_parsing() {
        let json = r#"{"default_generation_settings": {"params": {"n_ctx": 17152}}}"#;
        let props: LlamaPropsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(props.context_size(), Some(17152));
    }

    #[test]
    fn test_llama_props_ignores_theoretical_training_context() {
        let json = r#"{"n_ctx_train": 65536}"#;
        let props: LlamaPropsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(props.context_size(), None);
    }

    #[test]
    fn test_build_request_body_basic() {
        let request = ChatRequest {
            model: "test".into(),
            system_prompt: "You are a test".into(),
            messages: vec![],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
        };
        let body = build_request_body("test-model", request).unwrap();
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn test_build_request_body_uses_model_tool_content() {
        let request = ChatRequest {
            model: "test".into(),
            system_prompt: "You are a test".into(),
            messages: vec![ChatMessage::tool_result_with_model_content(
                "call-1",
                "full visible output",
                Some("limited model output"),
                false,
            )],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
        };

        let body = build_request_body("test-model", request).unwrap();
        assert_eq!(body["messages"][1]["content"], "limited model output");
    }

    #[tokio::test]
    async fn test_mock_provider_stream_chat() {
        let provider = MockProvider::new(vec![vec![StreamEvent::ContentBlockDelta {
            index: 0,
            delta: ContentBlockDelta::TextDelta {
                text: "hello".into(),
            },
        }]]);

        let request = ChatRequest {
            model: "test".into(),
            system_prompt: "test".into(),
            messages: vec![],
            tools: vec![],
            tool_choice: ToolChoice::Auto,
            max_tokens: None,
            temperature: None,
        };

        let mut stream = provider.stream_chat(request).await.unwrap();
        let event = stream.next().await.unwrap();
        match event {
            StreamEvent::ContentBlockDelta {
                delta: ContentBlockDelta::TextDelta { text },
                ..
            } => {
                assert_eq!(text, "hello");
            }
            _ => panic!("Expected text delta, got {:?}", event),
        }
    }

    #[tokio::test]
    async fn test_mock_provider_context_window() {
        let provider = MockProvider::new(vec![]);
        assert_eq!(provider.context_window().await.unwrap(), 4096);
    }

    #[tokio::test]
    async fn test_mock_provider_list_models() {
        let provider = MockProvider::new(vec![]);
        let models = provider.list_models().await.unwrap();
        assert_eq!(models, vec!["mock-model"]);
    }
}
