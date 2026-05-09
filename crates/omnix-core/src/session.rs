use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use omnix_protocol::{ChatMessage, ContentBlock, Role};

/// Summary of a compaction operation.
pub struct CompactionResult {
    pub summary: String,
    pub removed_count: usize,
}

/// In-memory conversation session with disk persistence.
pub struct Session {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub model: String,
    pub messages: Vec<ChatMessage>,
}

/// Lightweight metadata for listing sessions without loading full history.
pub struct SessionMetadata {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub message_count: usize,
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
    },
    Message(ChatMessage),
}

impl Session {
    /// Create a new blank session.
    pub fn new(model: impl Into<String>) -> Self {
        let now = Utc::now();
        let id = format!("session-{}", now.timestamp_millis());
        Self {
            id,
            created_at: now,
            updated_at: now,
            model: model.into(),
            messages: Vec::new(),
        }
    }

    /// Append a message and bump the updated timestamp.
    pub fn push_message(&mut self, message: ChatMessage) {
        self.messages.push(message);
        self.updated_at = Utc::now();
    }

    /// Estimate total tokens in session (heuristic: chars / 4).
    pub fn estimated_tokens(&self) -> usize {
        let total_chars: usize = self.messages.iter()
            .flat_map(|m| &m.content)
            .map(|block| match block {
                ContentBlock::Text { text } => text.len(),
                ContentBlock::ToolUse { id, name, input } => {
                    id.len() + name.len() + input.to_string().len()
                }
                ContentBlock::ToolResult { tool_use_id, content, .. } => {
                    tool_use_id.len() + content.len()
                }
                ContentBlock::Thinking { thinking } => thinking.len(),
            })
            .sum();
        (total_chars / 4).max(1)
    }

    /// Find the index where to split messages for compaction.
    /// Keeps the last `keep_turns` complete user-assistant cycles.
    /// Returns the index of the first message to KEEP (0 if nothing to compact).
    pub fn find_split_index(&self, keep_turns: usize) -> usize {
        if self.messages.len() < 2 {
            return 0;
        }

        // Walk backwards to find turn boundaries
        let mut turns_found = 0;
        let mut split_idx = 0;

        for (i, msg) in self.messages.iter().enumerate().rev() {
            if msg.role == Role::User {
                turns_found += 1;
                if turns_found >= keep_turns {
                    split_idx = i;
                    break;
                }
            }
        }

        // If we didn't find enough turns, keep everything
        if turns_found < keep_turns {
            return 0;
        }

        // Make sure we don't split in the middle of a tool call pair
        // If split_idx points to a tool result, move back to include its tool_use
        if split_idx > 0
            && let Some(msg) = self.messages.get(split_idx)
            && msg.role == Role::Tool
        {
            // Find the preceding assistant message with matching tool_use
            for j in (0..split_idx).rev() {
                if self.messages[j].role == Role::Assistant {
                    split_idx = j;
                    break;
                }
            }
        }

        split_idx
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
    pub fn save(&self) -> Result<()> {
        let dir = Self::sessions_dir()?;
        fs::create_dir_all(&dir)?;

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
    pub fn load(id: &str) -> Result<Self> {
        let path = Self::sessions_dir()?.join(format!("{}.jsonl", id));
        Self::load_from_path(&path)
    }

    /// List all saved sessions, sorted by most recently updated first.
    pub fn list() -> Result<Vec<SessionMetadata>> {
        let dir = Self::sessions_dir()?;
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();

        for entry in fs::read_dir(&dir)? {
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
                });
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(sessions)
    }

    // Helpers
    fn load_from_path(path: &PathBuf) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Failed to open: {:?}", path))?;
        let reader = BufReader::new(file);

        let mut id = None;
        let mut created_at = None;
        let mut updated_at = None;
        let mut model = None;
        let mut messages = Vec::new();

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
                    ..
                } => {
                    id = Some(i);
                    created_at = Some(c);
                    updated_at = Some(u);
                    model = Some(m);
                }
                SessionRecord::Message(msg) => messages.push(msg),
            }
        }

        Ok(Session {
            id: id.context("Missing session_meta.id")?,
            created_at: created_at.context("Missing session_meta.created_at")?,
            updated_at: updated_at.context("Missing session_meta.updated_at")?,
            model: model.context("Missing session_meta.model")?,
            messages,
        })
    }

    fn sessions_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(".omnix").join("sessions"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnix_protocol::ChatMessage;

    #[test]
    fn test_estimated_tokens_basic() {
        let mut session = Session::new("test");
        session.push_message(ChatMessage::user("Hello world"));
        // "Hello world" = 11 chars / 4 = ~3 tokens, but max(1) ensures at least 1
        let tokens = session.estimated_tokens();
        assert!(tokens >= 1);
    }

    #[test]
    fn test_estimated_tokens_empty() {
        let session = Session::new("test");
        assert_eq!(session.estimated_tokens(), 1); // max(1)
    }

    #[test]
    fn test_find_split_index_basic() {
        let mut session = Session::new("test");
        // Add 3 turns: user1, assistant1, user2, assistant2, user3, assistant3
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));
        session.push_message(ChatMessage::user("Turn 2"));
        session.push_message(ChatMessage::assistant_text("Response 2"));
        session.push_message(ChatMessage::user("Turn 3"));
        session.push_message(ChatMessage::assistant_text("Response 3"));

        // Keep last 2 turns -> should split at index 2 (start of turn 2)
        let split = session.find_split_index(2);
        assert_eq!(split, 2);
    }

    #[test]
    fn test_find_split_index_keep_all() {
        let mut session = Session::new("test");
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));

        // Request to keep 5 turns but only have 1 -> return 0 (keep all)
        let split = session.find_split_index(5);
        assert_eq!(split, 0);
    }

    #[test]
    fn test_find_split_index_tool_pair_protection() {
        let mut session = Session::new("test");
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));
        session.push_message(ChatMessage::user("Turn 2"));
        // Assistant with tool call
        let mut tool_msg = ChatMessage::assistant_text("Using tool");
        tool_msg.content.push(ContentBlock::ToolUse {
            id: "tool-1".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "ls"}),
        });
        session.push_message(tool_msg);
        session.push_message(ChatMessage::tool_result("tool-1", "file.txt", false));
        session.push_message(ChatMessage::user("Turn 3"));
        session.push_message(ChatMessage::assistant_text("Response 3"));

        // Keep last 1 turn -> split should protect the tool pair
        let split = session.find_split_index(1);
        // Should split at the user message of turn 3 (index 5)
        assert_eq!(split, 5);
    }

    #[test]
    fn test_compact_basic() {
        let mut session = Session::new("test");
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
        let mut session = Session::new("test");
        session.push_message(ChatMessage::user("Turn 1"));
        session.push_message(ChatMessage::assistant_text("Response 1"));

        // split_idx = 0 means keep everything
        let removed = session.compact(0, "Summary".into());
        assert_eq!(removed, 0);
        assert_eq!(session.messages.len(), 2);
    }

    #[test]
    fn test_compact_preserves_kept_messages() {
        let mut session = Session::new("test");
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
}
