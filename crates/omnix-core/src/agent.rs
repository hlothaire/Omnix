use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use futures::StreamExt;
use omnix_protocol::{
    ApprovalResponse, ChatMessage, ContentBlock, CoreCommand, CoreEvent, Role, StopReason,
    TokenUsage, ToolChoice,
};
use tokio::sync::mpsc;

use crate::memory_store::MemoryStore;
use crate::permissions::{AuthResult, PermissionEnforcer};
use crate::prompt::SystemPromptBuilder;
use crate::provider::{ChatRequest, ContentBlockDelta, Provider, StreamEvent};
use crate::session::Session;
use crate::telemetry::TelemetrySink;
use crate::tools::{ToolContext, ToolRegistry};

/// The central agent orchestrator.
///
/// Receives `CoreCommand`s via an async channel, runs the ReAct agentic loop,
/// and emits `CoreEvent`s back to the UI.
pub struct AgentCore<P: Provider> {
    provider: P,
    session: Session,
    tools: ToolRegistry,
    permissions: PermissionEnforcer,
    memory_store_path: PathBuf,
    event_tx: mpsc::UnboundedSender<CoreEvent>,
    telemetry: Option<TelemetrySink>,
    max_iterations: usize,
}

impl<P: Provider> AgentCore<P> {
    pub fn new(
        provider: P,
        model: String,
        tools: ToolRegistry,
        permissions: PermissionEnforcer,
        _prompt_builder: SystemPromptBuilder,
        memory_store_path: PathBuf,
        event_tx: mpsc::UnboundedSender<CoreEvent>,
    ) -> Self {
        Self {
            provider,
            session: Session::new(model),
            tools,
            permissions,
            memory_store_path,
            event_tx,
            telemetry: None,
            max_iterations: 25,
        }
    }

    pub fn with_telemetry(mut self, sink: TelemetrySink) -> Self {
        self.telemetry = Some(sink);
        self
    }

    /// Run the main command loop. Blocks until `Shutdown` is received.
    pub async fn run(&mut self, mut command_rx: mpsc::UnboundedReceiver<CoreCommand>) {
        while let Some(cmd) = command_rx.recv().await {
            match cmd {
                CoreCommand::SendPrompt { text } => {
                    self.execute_prompt(text, &mut command_rx).await;
                }
                CoreCommand::NewSession => {
                    self.session = Session::new(self.session.model.clone());
                    let _ = self.event_tx.send(CoreEvent::SessionCreated {
                        id: self.session.id.clone(),
                    });
                }
                CoreCommand::SaveSession => match self.session.save() {
                    Ok(()) => {
                        let _ = self.event_tx.send(CoreEvent::SessionSaved {
                            id: self.session.id.clone(),
                        });
                    }
                    Err(e) => {
                        let _ = self.event_tx.send(CoreEvent::ApiError {
                            message: e.to_string(),
                            retryable: false,
                        });
                    }
                },
                CoreCommand::LoadSession { id } => match Session::load(&id) {
                    Ok(session) => {
                        let msgs = session.messages.clone();
                        self.session = session;
                        let _ = self
                            .event_tx
                            .send(CoreEvent::SessionLoaded { id, messages: msgs });
                    }
                    Err(e) => {
                        let _ = self.event_tx.send(CoreEvent::ApiError {
                            message: e.to_string(),
                            retryable: false,
                        });
                    }
                },
                CoreCommand::SetPermissionMode { mode } => {
                    self.permissions.set_mode(mode);
                }
                CoreCommand::Shutdown => break,
                _ => {}
            }
        }
    }

    async fn execute_prompt(
        &mut self,
        prompt: String,
        command_rx: &mut mpsc::UnboundedReceiver<CoreCommand>,
    ) {
        let session_id = self.session.id.clone();
        if let Some(ref telemetry) = self.telemetry {
            telemetry.log_prompt(&session_id, &prompt);
        }

        self.session.push_message(ChatMessage::user(prompt));

        let mut iterations = 0;

        loop {
            iterations += 1;
            if iterations > self.max_iterations {
                let _ = self.event_tx.send(CoreEvent::MaxIterationsReached {
                    limit: self.max_iterations,
                });
                let msg = format!(
                    "Agent stopped after {} iterations to prevent runaway loops. \
                     Consider breaking your request into smaller steps.",
                    self.max_iterations
                );
                if let Some(ref telemetry) = self.telemetry {
                    telemetry.log_error(&self.session.id, &msg, false);
                }
                let _ = self.event_tx.send(CoreEvent::ApiError {
                    message: msg,
                    retryable: false,
                });
                break;
            }

            // Check if compaction is needed
            if let Ok(context_window) = self.provider.context_window().await {
                let threshold = (context_window * 75) / 100;
                if self.session.estimated_tokens() > threshold {
                    let split_idx = self.session.find_split_index(6);
                    if split_idx > 0 {
                        // Serialize old messages for summarization
                        let old_messages: Vec<ChatMessage> = self.session.messages[..split_idx].to_vec();
                        let conversation_text = old_messages.iter()
                            .map(|m| format!("{:?}: {:?}", m.role, m.content))
                            .collect::<Vec<_>>()
                            .join("\n\n");

                        let summary_prompt = format!(
                            "Summarize the following conversation concisely. Preserve key facts, decisions, file paths, errors, and next steps.\n\n{}",
                            conversation_text
                        );

                        match self.provider.summarize(summary_prompt, 512).await {
                            Ok(summary) => {
                                let removed = self.session.compact(split_idx, summary.clone());
                                let _ = self.event_tx.send(CoreEvent::SessionCompacted {
                                    summary: summary.clone(),
                                    removed_count: removed,
                                });
                                if let Some(ref telemetry) = self.telemetry {
                                    telemetry.log_compaction(&self.session.id, removed, &summary);
                                }
                            }
                            Err(e) => {
                                let _ = self.event_tx.send(CoreEvent::ApiError {
                                    message: format!("Compaction failed: {}", e),
                                    retryable: false,
                                });
                            }
                        }
                    }
                }
            }

            let memory_snapshot = MemoryStore::load(&self.memory_store_path)
                .ok()
                .and_then(|m| {
                    let s = m.format_for_prompt();
                    if s.is_empty() { None } else { Some(s) }
                });

            let prompt_builder = SystemPromptBuilder::new(self.permissions.mode())
                .with_tools(self.tools.tool_definitions())
                .with_memory_snapshot(memory_snapshot);

            let system_prompt = prompt_builder.build();

            let request = ChatRequest {
                model: self.session.model.clone(),
                system_prompt,
                messages: self.session.messages.clone(),
                tools: self.tools.tool_definitions(),
                tool_choice: ToolChoice::Auto,
                max_tokens: Some(4096),
                temperature: Some(0.7),
            };

            let _ = self.event_tx.send(CoreEvent::TurnStarted {
                model: self.session.model.clone(),
            });

            let mut stream = match self.provider.stream_chat(request).await {
                Ok(s) => s,
                Err(e) => {
                    if let Some(ref telemetry) = self.telemetry {
                        telemetry.log_error(&self.session.id, &e.to_string(), true);
                    }
                    let _ = self.event_tx.send(CoreEvent::ApiError {
                        message: e.to_string(),
                        retryable: true,
                    });
                    break;
                }
            };

            let mut text_buffer = String::new();
            let mut tool_meta: HashMap<usize, (String, String)> = HashMap::new();
            let mut tool_json: HashMap<usize, String> = HashMap::new();

            while let Some(event) = stream.next().await {
                match event {
                    StreamEvent::ContentBlockDelta {
                        delta: ContentBlockDelta::TextDelta { text },
                        ..
                    } => {
                        text_buffer.push_str(&text);
                        let _ = self.event_tx.send(CoreEvent::TokenDelta { text });
                    }
                    StreamEvent::ToolCallMeta { index, id, name } => {
                        tool_meta.insert(index, (id, name));
                    }
                    StreamEvent::ContentBlockDelta {
                        index,
                        delta: ContentBlockDelta::InputJsonDelta { partial_json },
                    } => {
                        tool_json.entry(index).or_default().push_str(&partial_json);
                    }
                    StreamEvent::MessageDelta {
                        stop_reason: Some(reason),
                        ..
                    } => {
                        let mut content = vec![ContentBlock::Text {
                            text: text_buffer.clone(),
                        }];

                        for (idx, json_str) in &tool_json {
                            if let Some((id, name)) = tool_meta.get(idx) {
                                match serde_json::from_str(json_str) {
                                    Ok(input) => {
                                        content.push(ContentBlock::ToolUse {
                                            id: id.clone(),
                                            name: name.clone(),
                                            input,
                                        });
                                    }
                                    Err(e) => {
                                        let _ = self.event_tx.send(CoreEvent::ToolError {
                                            call_id: id.clone(),
                                            message: format!("Malformed tool input JSON: {}", e),
                                        });
                                    }
                                }
                            }
                        }

                        self.session.push_message(ChatMessage {
                            role: Role::Assistant,
                            content,
                            usage: None,
                            stop_reason: Some(reason.clone()),
                        });

                        if reason == StopReason::EndTurn {
                            let _ = self.event_tx.send(CoreEvent::TurnEnded {
                                stop_reason: reason,
                                usage: TokenUsage::zero(),
                            });
                            return;
                        }

                        if reason == StopReason::ToolUse {
                            break;
                        }
                    }
                    StreamEvent::Error { message } => {
                        if let Some(ref telemetry) = self.telemetry {
                            telemetry.log_error(&self.session.id, &message, true);
                        }
                        let _ = self.event_tx.send(CoreEvent::ApiError {
                            message,
                            retryable: true,
                        });
                        return;
                    }
                    _ => {}
                }
            }

            let ctx = ToolContext::default();
            let mut tool_results: Vec<(String, crate::tools::ToolOutput)> = Vec::new();

            for (idx, json_str) in &tool_json {
                let Some((id, name)) = tool_meta.get(idx) else {
                    continue;
                };

                let input: serde_json::Value = match serde_json::from_str(json_str) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = self.event_tx.send(CoreEvent::ToolError {
                            call_id: id.clone(),
                            message: format!("Invalid JSON: {}", e),
                        });
                        tool_results.push((
                            id.clone(),
                            crate::tools::ToolOutput::err(format!("Invalid JSON: {}", e)),
                        ));
                        continue;
                    }
                };

                let _ = self.event_tx.send(CoreEvent::ToolCallStarted {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                });
                if let Some(ref telemetry) = self.telemetry {
                    telemetry.log_tool_call(&self.session.id, name, &input);
                }

                match self.permissions.authorize(name, &input) {
                    AuthResult::Allow => {}
                    AuthResult::Deny { reason } => {
                        let output = crate::tools::ToolOutput::err(reason);
                        let _ = self.event_tx.send(CoreEvent::ToolCallCompleted {
                            id: id.clone(),
                            name: name.clone(),
                            output: omnix_protocol::ToolResultContent::err(output.content.clone()),
                        });
                        tool_results.push((id.clone(), output));
                        continue;
                    }
                    AuthResult::Ask {
                        description,
                        risk_level,
                    } => {
                        let _ = self.event_tx.send(CoreEvent::ApprovalRequested {
                            call_id: id.clone(),
                            tool_name: name.clone(),
                            tool_input: input.clone(),
                            risk_level,
                            description,
                        });

                        let response = self.wait_for_approval(id, command_rx).await;

                        match response {
                            ApprovalResponse::AllowOnce | ApprovalResponse::AllowForSession => {}
                            ApprovalResponse::Deny => {
                                let output = crate::tools::ToolOutput::err("User denied approval");
                                let _ = self.event_tx.send(CoreEvent::ToolCallCompleted {
                                    id: id.clone(),
                                    name: name.clone(),
                                    output: omnix_protocol::ToolResultContent::err(
                                        output.content.clone(),
                                    ),
                                });
                                tool_results.push((id.clone(), output));
                                continue;
                            }
                        }
                    }
                }

                let tool_start = Instant::now();
                match self.tools.execute(name, input, &ctx).await {
                    Ok(output) => {
                        let duration_ms = tool_start.elapsed().as_millis() as u64;
                        let _ = self.event_tx.send(CoreEvent::ToolCallCompleted {
                            id: id.clone(),
                            name: name.clone(),
                            output: omnix_protocol::ToolResultContent::ok(output.content.clone()),
                        });
                        if let Some(ref telemetry) = self.telemetry {
                            telemetry.log_tool_result(
                                &self.session.id,
                                name,
                                &output.content,
                                output.is_error,
                                duration_ms,
                            );
                        }
                        tool_results.push((id.clone(), output));
                    }
                    Err(e) => {
                        let duration_ms = tool_start.elapsed().as_millis() as u64;
                        let _ = self.event_tx.send(CoreEvent::ToolError {
                            call_id: id.clone(),
                            message: e.to_string(),
                        });
                        if let Some(ref telemetry) = self.telemetry {
                            telemetry.log_tool_result(
                                &self.session.id,
                                name,
                                &e.to_string(),
                                true,
                                duration_ms,
                            );
                        }
                        tool_results
                            .push((id.clone(), crate::tools::ToolOutput::err(e.to_string())));
                    }
                }
            }

            for (id, output) in tool_results {
                self.session.push_message(ChatMessage::tool_result(
                    id,
                    output.content,
                    output.is_error,
                ));
            }
        }
    }

    async fn wait_for_approval(
        &self,
        call_id: &str,
        command_rx: &mut mpsc::UnboundedReceiver<CoreCommand>,
    ) -> ApprovalResponse {
        while let Some(cmd) = command_rx.recv().await {
            match cmd {
                CoreCommand::RespondToApproval {
                    call_id: resp_id,
                    response,
                } => {
                    if resp_id == call_id {
                        return response;
                    }
                }
                CoreCommand::CancelTurn => {
                    return ApprovalResponse::Deny;
                }
                _ => {}
            }
        }
        ApprovalResponse::Deny
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::permissions::PermissionEnforcer;
    use crate::prompt::SystemPromptBuilder;
    use crate::provider::{LlamaCppProvider, MockProvider};
    use crate::tools::{Tool, ToolContext, ToolError, ToolOutput, ToolRegistry};
    use futures::future::BoxFuture;
    use omnix_protocol::{CoreCommand, CoreEvent, PermissionMode};
    use serde_json::json;

    fn setup_core_with_provider<P: Provider>(
        provider: P,
    ) -> (AgentCore<P>, mpsc::UnboundedReceiver<CoreEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let tools = ToolRegistry::new();
        let permissions = PermissionEnforcer::new(PermissionMode::Allow);
        let prompt = SystemPromptBuilder::new(PermissionMode::Allow);

        let core = AgentCore::new(
            provider,
            "test".into(),
            tools,
            permissions,
            prompt,
            PathBuf::from("/tmp/test_memory.md"),
            event_tx,
        );
        (core, event_rx)
    }

    fn setup_core() -> (
        AgentCore<LlamaCppProvider>,
        mpsc::UnboundedReceiver<CoreEvent>,
    ) {
        let provider = LlamaCppProvider::new("http://localhost:9999", "test-model");
        setup_core_with_provider(provider)
    }

    #[tokio::test]
    async fn agent_creates_session_on_new() {
        let (mut core, mut event_rx) = setup_core();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        cmd_tx.send(CoreCommand::NewSession).unwrap();
        cmd_tx.send(CoreCommand::Shutdown).unwrap();

        core.run(cmd_rx).await;

        let event = event_rx.recv().await.unwrap();
        assert!(matches!(event, CoreEvent::SessionCreated { .. }));
    }

    #[tokio::test]
    async fn agent_handles_shutdown() {
        let (mut core, _event_rx) = setup_core();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        cmd_tx.send(CoreCommand::Shutdown).unwrap();

        core.run(cmd_rx).await;
    }

    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn description(&self) -> &str {
            "Echo back the input message"
        }

        fn input_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                },
                "required": ["message"]
            })
        }

        fn required_permission(&self) -> PermissionMode {
            PermissionMode::ReadOnly
        }

        fn execute(
            &self,
            input: serde_json::Value,
            _ctx: &ToolContext,
        ) -> BoxFuture<'_, anyhow::Result<ToolOutput, ToolError>> {
            let msg = input["message"].as_str().unwrap_or("?").to_string();
            Box::pin(async move { Ok(ToolOutput::ok(format!("Echo: {}", msg))) })
        }
    }

    #[tokio::test]
    async fn react_loop_full_cycle() {
        // Turn 1: LLM emits text + tool call (echo), stops with ToolUse
        let turn1 = vec![
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentBlockDelta::TextDelta {
                    text: "I'll echo that.".into(),
                },
            },
            StreamEvent::ToolCallMeta {
                index: 0,
                id: "call_1".into(),
                name: "echo".into(),
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentBlockDelta::InputJsonDelta {
                    partial_json: r#"{"message": "hello"}"#.into(),
                },
            },
            StreamEvent::MessageDelta {
                stop_reason: Some(StopReason::ToolUse),
                usage: None,
            },
        ];

        // Turn 2: LLM sees tool result, responds with final text, stops with EndTurn
        let turn2 = vec![
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentBlockDelta::TextDelta {
                    text: "Done!".into(),
                },
            },
            StreamEvent::MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                usage: None,
            },
        ];

        let provider = MockProvider::new(vec![turn1, turn2]);
        let (mut core, mut event_rx) = setup_core_with_provider(provider);

        // Register echo tool
        core.tools.register(Arc::new(EchoTool));

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Send prompt and shutdown after the turn completes
        let shutdown_tx = cmd_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let _ = shutdown_tx.send(CoreCommand::Shutdown);
        });

        cmd_tx
            .send(CoreCommand::SendPrompt {
                text: "say hello".into(),
            })
            .unwrap();

        core.run(cmd_rx).await;

        // Collect all events
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }

        // Verify the sequence
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CoreEvent::TurnStarted { .. })),
            "Expected TurnStarted"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CoreEvent::TokenDelta { text } if text == "I'll echo that.")),
            "Expected TokenDelta with thinking text"
        );
        assert!(
            events.iter().any(|e| matches!(e, CoreEvent::ToolCallStarted { id, name, .. } if id == "call_1" && name == "echo")),
            "Expected ToolCallStarted for echo"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CoreEvent::ToolCallCompleted { id, .. } if id == "call_1")),
            "Expected ToolCallCompleted for echo"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CoreEvent::TokenDelta { text } if text == "Done!")),
            "Expected TokenDelta with final text"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                CoreEvent::TurnEnded {
                    stop_reason: StopReason::EndTurn,
                    ..
                }
            )),
            "Expected TurnEnded with EndTurn"
        );

        // Verify session state: user msg + assistant msg + tool result + assistant msg
        assert_eq!(core.session.messages.len(), 4);
        assert_eq!(core.session.messages[0].role, Role::User);
        assert_eq!(core.session.messages[1].role, Role::Assistant);
        assert_eq!(core.session.messages[2].role, Role::Tool);
        assert_eq!(core.session.messages[3].role, Role::Assistant);
    }

    #[tokio::test]
    async fn react_loop_approval_required() {
        // Turn 1: LLM calls a tool that requires approval
        let turn1 = vec![
            StreamEvent::ToolCallMeta {
                index: 0,
                id: "call_1".into(),
                name: "echo".into(),
            },
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentBlockDelta::InputJsonDelta {
                    partial_json: r#"{"message": "test"}"#.into(),
                },
            },
            StreamEvent::MessageDelta {
                stop_reason: Some(StopReason::ToolUse),
                usage: None,
            },
        ];

        // Turn 2: after approval + execution, LLM responds
        let turn2 = vec![
            StreamEvent::ContentBlockDelta {
                index: 0,
                delta: ContentBlockDelta::TextDelta {
                    text: "Approved!".into(),
                },
            },
            StreamEvent::MessageDelta {
                stop_reason: Some(StopReason::EndTurn),
                usage: None,
            },
        ];

        let provider = MockProvider::new(vec![turn1, turn2]);
        let (mut core, mut event_rx) = setup_core_with_provider(provider);

        // Set Prompt mode so all tools require approval
        core.permissions.set_mode(PermissionMode::Prompt);
        core.tools.register(Arc::new(EchoTool));

        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Approve after a short delay (deterministic timing in test)
        let cmd_tx2 = cmd_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            let _ = cmd_tx2.send(CoreCommand::RespondToApproval {
                call_id: "call_1".into(),
                response: ApprovalResponse::AllowOnce,
            });
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let _ = cmd_tx2.send(CoreCommand::Shutdown);
        });

        cmd_tx
            .send(CoreCommand::SendPrompt {
                text: "test".into(),
            })
            .unwrap();

        core.run(cmd_rx).await;

        // Collect events
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }

        assert!(
            events.iter().any(
                |e| matches!(e, CoreEvent::ApprovalRequested { call_id, .. } if call_id == "call_1")
            ),
            "Expected ApprovalRequested"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, CoreEvent::ToolCallCompleted { id, .. } if id == "call_1")),
            "Expected ToolCallCompleted after approval"
        );
    }
}
