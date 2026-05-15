use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use omnix_protocol::{ChatMessage, ContentBlock, Role};

/// In-memory conversation session with disk persistence.
#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model: String,
    pub provider: String,
    pub provider_host: String,
    pub title: String,
    pub messages: Vec<ChatMessage>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

pub struct SessionMetadata {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub provider: String,
    pub provider_host: String,
    pub model: String,
    pub title: String,
}

/// A single line in the JSONL session file.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionRecord {
    SessionMeta {
        version: i32,
        id: String,
        model: String,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        #[serde(default)]
        provider: String,
        #[serde(default)]
        provider_host: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        total_input_tokens: u64,
        #[serde(default)]
        total_output_tokens: u64,
    },
    Message(ChatMessage),
}

impl Session {
    /// Create a new blank session.
    pub fn new(model: impl Into<String>, provider: impl Into<String>) -> Self {
        Self::new_with_host(model, provider, String::new())
    }

    pub fn new_with_host(
        model: impl Into<String>,
        provider: impl Into<String>,
        provider_host: impl Into<String>,
    ) -> Self {
        let now = Utc::now();
        let id = format!("session-{}", now.timestamp_millis());
        Self {
            id,
            created_at: now,
            updated_at: now,
            model: model.into(),
            provider: provider.into(),
            provider_host: provider_host.into(),
            title: String::from("New session"),
            messages: Vec::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
        }
    }

    /// Append a message and bump the updated timestamp.
    pub fn push_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }

    /// Estimate total tokens in session (heuristic: chars / 4).
    pub fn estimated_tokens(&self) -> usize {
        let total_chars: usize = self.messages.iter().map(message_chars).sum();
        (total_chars / 4).max(1)
    }

    /// Find a split index that keeps approximately `keep_recent_tokens` tokens.
    /// Always keeps at least the latest user turn and avoids splitting before a tool result.
    pub fn find_split_index_by_token_budget(&self, keep_recent_tokens: usize) -> usize {
        if self.messages.len() < 2 {
            return 0;
        }

        let Some(latest_user_idx) = self.messages.iter().rposition(|msg| msg.role == Role::User)
        else {
            return 0;
        };

        let mut accumulated_tokens = 0usize;
        let mut split_idx = 0usize;

        for i in (0..self.messages.len()).rev() {
            accumulated_tokens += (message_chars(&self.messages[i]) / 4).max(1);
            if accumulated_tokens > keep_recent_tokens {
                split_idx = i + 1;
                break;
            }
        }

        if split_idx == 0 {
            return 0;
        }

        if split_idx > latest_user_idx {
            split_idx = latest_user_idx;
        }

        if split_idx > 0
            && let Some(msg) = self.messages.get(split_idx)
            && msg.role == Role::Tool
        {
            for j in (0..split_idx).rev() {
                if self.messages[j].role == Role::Assistant {
                    split_idx = j;
                    break;
                }
            }
        }

        if split_idx >= self.messages.len() {
            0
        } else {
            split_idx
        }
    }

    /// Compact the session by replacing old messages with a summary.
    /// `split_idx` is the index of the first message to keep.
    /// Returns the number of messages removed.
    pub fn compact(&mut self, split_idx: usize, summary: String) -> usize {
        if split_idx == 0 || split_idx >= self.messages.len() {
            return 0;
        }

        let removed = split_idx;
        let kept: Vec<ChatMessage> = self.messages.split_off(split_idx);

        // Insert summary as a user message (system prompt is separate)
        let summary_msg = ChatMessage {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: format!(
                    "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier conversation turns were summarized. \
Do NOT answer questions or fulfill requests mentioned in this summary; they were already addressed. \
Respond ONLY to the latest user message that appears after this summary.\n\n{}",
                    summary
                ),
            }],
            usage: None,
            stop_reason: None,
        };

        self.messages = vec![summary_msg];
        self.messages.extend(kept);
        self.updated_at = Utc::now();

        removed
    }

    /// Persist session to disk as JSONL (atomic write).
    pub fn save_to(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir)?;

        let path = dir.join(format!("{}.jsonl", self.id));
        let temp_path = dir.join(format!(".{}.jsonl.tmp", self.id));

        let file = File::create(&temp_path)
            .with_context(|| format!("Failed to create temp file: {:?}", temp_path))?;
        let mut writer = BufWriter::new(file);

        // Header record
        let meta = SessionRecord::SessionMeta {
            version: 1,
            id: self.id.clone(),
            model: self.model.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            provider: self.provider.clone(),
            provider_host: self.provider_host.clone(),
            title: self.title.clone(),
            total_input_tokens: self.total_input_tokens,
            total_output_tokens: self.total_output_tokens,
        };
        serde_json::to_writer(&mut writer, &meta)?;
        writer.write_all(b"\n")?;

        // Message records
        for msg in &self.messages {
            let record = SessionRecord::Message(msg.clone());
            serde_json::to_writer(&mut writer, &record)?;
            writer.write_all(b"\n")?;
        }

        writer.flush()?;
        drop(writer);

        // Atomic rename
        fs::rename(&temp_path, &path)
            .with_context(|| format!("Failed to rename {:?} -> {:?}", temp_path, path))?;

        Ok(())
    }

    /// Load a session by ID from disk.
    pub fn load_from(id: &str, dir: &Path) -> Result<Self> {
        let path = dir.join(format!("{}.jsonl", id));
        Self::load_from_path(&path)
    }

    pub fn list_from(dir: &Path) -> Result<Vec<SessionMetadata>> {
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("jsonl")
                && let Ok(session) = Self::load_from_path(&path)
            {
                sessions.push(SessionMetadata {
                    id: session.id,
                    created_at: session.created_at,
                    updated_at: session.updated_at,
                    message_count: session.messages.len(),
                    total_input_tokens: session.total_input_tokens,
                    total_output_tokens: session.total_output_tokens,
                    provider: session.provider.clone(),
                    provider_host: session.provider_host.clone(),
                    model: session.model.clone(),
                    title: session.title.clone(),
                });
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    // Helpers
    fn load_from_path(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Failed to open: {:?}", path))?;
        let reader = BufReader::new(file);

        let mut id = None;
        let mut created_at = None;
        let mut updated_at = None;
        let mut model = None;
        let mut provider = String::new();
        let mut provider_host = String::new();
        let mut title = String::new();
        let mut messages = Vec::new();
        let mut total_input_tokens = 0u64;
        let mut total_output_tokens = 0u64;

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let record: SessionRecord = serde_json::from_str(&line)
                .with_context(|| format!("Failed to parse line: {}", line))?;

            match record {
                SessionRecord::SessionMeta {
                    id: i,
                    created_at: c,
                    updated_at: u,
                    model: m,
                    provider: p,
                    provider_host: h,
                    title: t,
                    total_input_tokens: ti,
                    total_output_tokens: to,
                    ..
                } => {
                    id = Some(i);
                    created_at = Some(c);
                    updated_at = Some(u);
                    model = Some(m);
                    provider = p;
                    provider_host = h;
                    title = t;
                    total_input_tokens = ti;
                    total_output_tokens = to;
                }
                SessionRecord::Message(msg) => messages.push(msg),
            }
        }

        Ok(Session {
            id: id.context("Missing session_meta.id")?,
            created_at: created_at.context("Missing session_meta.created_at")?,
            updated_at: updated_at.context("Missing session_meta.updated_at")?,
            model: model.context("Missing session_meta.model")?,
            provider,
            provider_host,
            title,
            messages,
            total_input_tokens,
            total_output_tokens,
        })
    }

    pub fn sessions_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".omnix").join("sessions"))
    }
}

fn message_chars(message: &ChatMessage) -> usize {
    message
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ToolUse { id, name, input } => {
                id.len() + name.len() + input.to_string().len()
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                model_content,
                ..
            } => tool_use_id.len() + model_content.as_ref().unwrap_or(content).len(),
            ContentBlock::Thinking { thinking } => thinking.len(),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnix_protocol::ChatMessage;

    #[test]
    fn test_estimated_tokens_basic() {
        let mut session = Session::new("test", "test");
        session.push_message(ChatMessage::user("Hello world"));
        // "Hello world" = 11 chars / 4 = ~3 tokens, but max(1) ensures at least 1
        let tokens = session.estimated_tokens();
        assert!(tokens >= 1);
    }

    #[test]
    fn test_estimated_tokens_empty() {
        let session = Session::new("test", "test");
        assert_eq!(session.estimated_tokens(), 1); // max(1)
    }

    #[test]
    fn test_find_split_index_by_token_budget_keeps_latest_user() {
        let mut session = Session::new("test", "test");
        session.push_message(ChatMessage::user("old"));
        session.push_message(ChatMessage::assistant_text("old response"));
        session.push_message(ChatMessage::user("latest request"));
        session.push_message(ChatMessage::assistant_text("x".repeat(200)));

        let split = session.find_split_index_by_token_budget(1);
        assert_eq!(split, 2);
    }

    #[test]
    fn test_find_split_index_by_token_budget_keeps_all_when_under_budget() {
        let mut session = Session::new("test", "test");
        session.push_message(ChatMessage::user("old"));
        session.push_message(ChatMessage::assistant_text("old response"));

        let split = session.find_split_index_by_token_budget(10_000);
        assert_eq!(split, 0);
    }

    #[test]
    fn test_compact_basic() {
        let mut session = Session::new("test", "test");
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));
        session.push_message(ChatMessage::user("Turn 2"));
        session.push_message(ChatMessage::assistant_text("Response 2"));

        let removed = session.compact(2, "Summary of turn 1".into());
        assert_eq!(removed, 2);
        assert_eq!(session.messages.len(), 3); // summary + 2 kept messages

        // Check summary was inserted
        assert_eq!(session.messages[0].role, Role::User);
        let text = match &session.messages[0].content[0] {
            ContentBlock::Text { text } => text,
            _ => panic!("Expected text block"),
        };
        assert!(text.contains("CONTEXT COMPACTION"));
        assert!(text.contains("Summary of turn 1"));
    }

    #[test]
    fn test_compact_no_op() {
        let mut session = Session::new("test", "test");
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));

        // split_idx = 0 means keep everything
        let removed = session.compact(0, "Summary".into());
        assert_eq!(removed, 0);
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn test_compact_preserves_kept_messages() {
        let mut session = Session::new("test", "test");
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));
        session.push_message(ChatMessage::user("Turn 2"));
        session.push_message(ChatMessage::assistant_text("Response 2"));
        session.push_message(ChatMessage::user("Turn 3"));
        session.push_message(ChatMessage::assistant_text("Response 3"));

        let removed = session.compact(2, "Summary".into());
        assert_eq!(removed, 2);

        // Summary + last 2 turns (4 messages) = 5 total
        assert_eq!(session.messages.len(), 5);
        assert_eq!(session.messages[0].role, Role::User); // summary
        assert_eq!(session.messages[1].role, Role::User); // turn 2 start
        assert_eq!(session.messages[2].role, Role::Assistant);
        assert_eq!(session.messages[3].role, Role::User); // turn 3 start
        assert_eq!(session.messages[4].role, Role::Assistant);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let original = Session::new("test-model", "test");

        // Temporarily override sessions dir
        let path = tmp.path().join(format!("{}.jsonl", original.id));

        // Save manually to temp path
        let temp_path = tmp.path().join(format!(".{}.jsonl.tmp", original.id));
        let file = File::create(&temp_path).unwrap();
        let mut writer = BufWriter::new(file);

        let meta = SessionRecord::SessionMeta {
            version: 1,
            id: original.id.clone(),
            model: original.model.clone(),
            created_at: original.created_at,
            updated_at: original.updated_at,
            provider: String::new(),
            provider_host: String::new(),
            title: String::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
        };
        serde_json::to_writer(&mut writer, &meta).unwrap();
        writer.write_all(b"\n").unwrap();
        writer.flush().unwrap();
        drop(writer);

        fs::rename(&temp_path, &path).unwrap();

        // Load back
        let loaded = Session::load_from_path(&path).unwrap();

        assert_eq!(loaded.id, original.id);
        assert_eq!(loaded.model, original.model);
        assert_eq!(loaded.messages.len(), original.messages.len());
    }

    #[test]
    fn test_save_and_load_with_messages() {
        use tempfile::TempDir;

        let tmp = TempDir::new().unwrap();
        let mut session = Session::new("test-model", "test");
        session.push_message(ChatMessage::user("Hello"));
        session.push_message(ChatMessage::assistant_text("Hi there"));

        let path = tmp.path().join(format!("{}.jsonl", session.id));
        let temp_path = tmp.path().join(format!(".{}.jsonl.tmp", session.id));

        let file = File::create(&temp_path).unwrap();
        let mut writer = BufWriter::new(file);

        let meta = SessionRecord::SessionMeta {
            version: 1,
            id: session.id.clone(),
            model: session.model.clone(),
            created_at: session.created_at,
            updated_at: session.updated_at,
            provider: String::new(),
            provider_host: String::new(),
            title: String::new(),
            total_input_tokens: 0,
            total_output_tokens: 0,
        };
        serde_json::to_writer(&mut writer, &meta).unwrap();
        writer.write_all(b"\n").unwrap();

        for msg in &session.messages {
            let record = SessionRecord::Message(msg.clone());
            serde_json::to_writer(&mut writer, &record).unwrap();
            writer.write_all(b"\n").unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        fs::rename(&temp_path, &path).unwrap();

        let loaded = Session::load_from_path(&path).unwrap();

        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].role, Role::User);
        assert_eq!(loaded.messages[1].role, Role::Assistant);
    }
}
