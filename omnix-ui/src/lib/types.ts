export type PermissionMode = "ReadOnly" | "WorkspaceWrite" | "DangerFullAccess" | "Prompt" | "Allow";
export type ApprovalResponse = "AllowOnce" | "AllowForSession" | "Deny";
export type RiskLevel = "Informational" | "WorkspaceWrite" | "Destructive";
export type StopReason = "EndTurn" | "ToolUse" | "MaxTokens";

export interface DisplayMessage {
  role: "User" | "Assistant" | "Tool";
  text: string;
  thinking: string;
  toolCalls: ToolCallDisplay[];
  isError: boolean;
  time: string;
}

export interface CompactionNotice {
  removedCount: number;
  tokensBefore: number;
  inputBudget: number;
  firstKeptIndex: number;
  createdAt: string;
}

export interface ToolCallDisplay {
  callId: string;
  name: string;
  input: Record<string, unknown>;
  output?: string;
  isError: boolean;
}

export interface PendingApproval {
  sessionId: string;
  callId: string;
  toolName: string;
  toolInput: Record<string, unknown>;
  riskLevel: string;
  description: string;
}

export interface SessionInfo {
  id: string;
  title: string;
  updatedAt: string;
  provider: string;
  host: string;
  model: string;
}

export interface SessionListEntry {
  id: string;
  updated_at: string;
  message_count: number;
  total_input_tokens: number;
  total_output_tokens: number;
  provider: string;
  host: string;
  model: string;
  title: string;
}

export type CoreEvent =
  | { event: "token_delta"; session_id: string; text: string }
  | { event: "thinking_delta"; session_id: string; thinking: string }
  | { event: "tool_call_started"; session_id: string; id: string; name: string; input: Record<string, unknown> }
  | { event: "tool_call_completed"; session_id: string; id: string; name: string; output: { output: string; is_error: boolean } }
  | { event: "tool_error"; session_id: string; call_id: string; message: string }
  | { event: "turn_started"; session_id: string; model: string }
  | { event: "turn_ended"; session_id: string; stop_reason: StopReason; usage: { input_tokens: number; output_tokens: number } }
  | { event: "approval_requested"; session_id: string; call_id: string; tool_name: string; tool_input: Record<string, unknown>; risk_level: RiskLevel; description: string }
  | { event: "session_created"; id: string; provider: string; host: string; model: string }
  | { event: "session_loaded"; id: string; messages: ChatMessage[]; total_input_tokens: number; total_output_tokens: number; title: string; provider: string; host: string; model: string }
  | { event: "session_saved"; id: string }
  | { event: "session_deleted"; id: string }
  | { event: "session_listed"; sessions: SessionListEntry[] }
  | { event: "session_compacted"; session_id: string; summary: string; removed_count: number; messages: ChatMessage[]; tokens_before: number; input_budget: number; first_kept_index: number }
  | { event: "api_error"; session_id?: string | null; message: string; retryable: boolean }
  | { event: "fatal_error"; message: string }
  | { event: "max_iterations_reached"; limit: number }
  | { event: "context_updated"; session_id: string; used_tokens: number; max_tokens: number | null; percent: number | null }
  | { event: "model_changed"; model: string }
  | { event: "provider_status_changed"; provider: string; host: string; connected: boolean }
  | { event: "memory_added"; target: string; content: string; usage: string }
  | { event: "memory_replaced"; target: string; old_text: string; new_text: string; usage: string }
  | { event: "memory_removed"; target: string; old_text: string; usage: string }
  | { event: "usage_updated"; tokens_in: number; tokens_out: number; cost_usd: number };

export interface ChatMessage {
  role: "User" | "Assistant" | "Tool";
  content: ContentBlock[];
}

export type ContentBlock =
  | { type: "text"; text: string }
  | { type: "tool_use"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_result"; tool_use_id: string; content: string; model_content?: string | null; is_error: boolean }
  | { type: "thinking"; thinking: string };

export function convertChatMessages(messages: ChatMessage[]): DisplayMessage[] {
  const displayMessages: DisplayMessage[] = [];

  for (const m of messages) {
    const textParts: string[] = [];
    const thinkingParts: string[] = [];
    const toolCalls: ToolCallDisplay[] = [];

    for (const block of m.content) {
      switch (block.type) {
        case "text":
          textParts.push(block.text);
          break;
        case "thinking":
          thinkingParts.push(block.thinking);
          break;
        case "tool_use":
          toolCalls.push({
            callId: block.id,
            name: block.name,
            input: block.input,
            isError: false,
          });
          break;
        case "tool_result":
          if (m.role === "Tool") {
            const owner = [...displayMessages]
              .reverse()
              .find((msg) => msg.toolCalls.some((tc) => tc.callId === block.tool_use_id));
            const toolCall = owner?.toolCalls.find((tc) => tc.callId === block.tool_use_id);
            if (toolCall) {
              toolCall.output = block.content;
              toolCall.isError = block.is_error;
            }
          } else {
            const toolCall = toolCalls.find((tc) => tc.callId === block.tool_use_id);
            if (toolCall) {
              toolCall.output = block.content;
              toolCall.isError = block.is_error;
            }
          }
          break;
      }
    }

    if (m.role === "Tool" && textParts.length === 0 && thinkingParts.length === 0 && toolCalls.length === 0) {
      continue;
    }

    displayMessages.push({
      role: m.role,
      text: textParts.join("\n"),
      thinking: thinkingParts.join(""),
      toolCalls,
      isError: false,
      time: new Date().toLocaleTimeString(),
    });
  }

  return displayMessages;
}
