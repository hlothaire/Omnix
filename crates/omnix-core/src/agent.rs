use std::collections::HashMap;
use std::path::PathBuf;

use futures::StreamExt;
use omnix_protocol::{
    ApprovalResponse, ChatMessage, ContentBlock, CoreCommand, CoreEvent, Role, StopReason,
    TokenUsage, ToolChoice,
};
use tokio::sync::mpsc;

use crate::memory_store::MemoryStore;
use crate::permissions::{AuthResult, PermissionEnforcer};
use crate::prompt::SystemPromptBuilder;
use crate::provider::{ChatRequest, ContentBlockDelta, LlamaCppProvider, StreamEvent};
use crate::session::Session;
use crate::tools::{ToolContext, ToolRegistry};

/// The central agent orchestrator.
///
/// Receives `CoreCommand`s via an async channel, runs the ReAct agentic loop,
/// and emits `CoreEvent`s back to the UI.
pub struct AgentCore {
    provider: LlamaCppProvider,
    session: Session,
    tools: ToolRegistry,
    permissions: PermissionEnforcer,
    memory_store_path: PathBuf,
    event_tx: mpsc::UnboundedSender<CoreEvent>,
    max_iterations: usize,
}

impl AgentCore {
    pub fn new(
        provider: LlamaCppProvider,
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
            max_iterations: 25,
        }
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
                // Other commands are handled inside execute_prompt or ignored at top level
                _ => {}
            }
        }
    }

    async fn execute_prompt(
        &mut self,
        prompt: String,
        command_rx: &mut mpsc::UnboundedReceiver<CoreCommand>,
    ) {
        self.session.push_message(ChatMessage::user(prompt));

        let mut iterations = 0;

        loop {
            iterations += 1;
            if iterations > self.max_iterations {
                let _ = self.event_tx.send(CoreEvent::ApiError {
                    message: "Max iterations reached".into(),
                    retryable: false,
                });
                break;
            }

            // Build system prompt with current memory snapshot
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
                    let _ = self.event_tx.send(CoreEvent::ApiError {
                        message: e.to_string(),
                        retryable: true,
                    });
                    break;
                }
            };

            // Accumulate the assistant response
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
                        // Build assistant message content
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
                            break; // Execute tools below
                        }
                    }
                    StreamEvent::Error { message } => {
                        let _ = self.event_tx.send(CoreEvent::ApiError {
                            message,
                            retryable: true,
                        });
                        return;
                    }
                    _ => {}
                }
            }

            // Execute collected tool calls
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

                // Permission check
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

                // Execute
                match self.tools.execute(name, input, &ctx).await {
                    Ok(output) => {
                        let _ = self.event_tx.send(CoreEvent::ToolCallCompleted {
                            id: id.clone(),
                            name: name.clone(),
                            output: omnix_protocol::ToolResultContent::ok(output.content.clone()),
                        });
                        tool_results.push((id.clone(), output));
                    }
                    Err(e) => {
                        let _ = self.event_tx.send(CoreEvent::ToolError {
                            call_id: id.clone(),
                            message: e.to_string(),
                        });
                        tool_results
                            .push((id.clone(), crate::tools::ToolOutput::err(e.to_string())));
                    }
                }
            }

            // Append tool results to session
            for (id, output) in tool_results {
                self.session.push_message(ChatMessage::tool_result(
                    id,
                    output.content,
                    output.is_error,
                ));
            }

            // Loop back for next LLM turn
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
    use super::*;
    use crate::permissions::PermissionEnforcer;
    use crate::prompt::SystemPromptBuilder;
    use crate::tools::ToolRegistry;
    use omnix_protocol::{CoreCommand, CoreEvent, PermissionMode};

    fn setup_core() -> (AgentCore, mpsc::UnboundedReceiver<CoreEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let provider = LlamaCppProvider::new("http://localhost:9999", "test-model");
        let tools = ToolRegistry::new();
        let permissions = PermissionEnforcer::new(PermissionMode::Allow);
        let prompt = SystemPromptBuilder::new(PermissionMode::Allow);

        let core = AgentCore::new(
            provider,
            "test".into(),
            tools,
            permissions,
            prompt, // passed but ignored internally
            PathBuf::from("/tmp/test_memory.md"),
            event_tx,
        );
        (core, event_rx)
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
        // Should exit cleanly
    }
}
