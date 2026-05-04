use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use omnix_protocol::ChatMessage;

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
