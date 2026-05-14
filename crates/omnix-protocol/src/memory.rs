use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum MemoryResult {
    Success {
        message: String,
        new_usage: String,
    },
    CapacityExceeded {
        current_chars: usize,
        limit: usize,
        entry_chars: usize,
        current_entries: Vec<String>,
    },
    Duplicate {
        message: String,
    },
    NotFound {
        message: String,
    },
    MultipleMatch {
        matches: Vec<String>,
        message: String,
    },
    SecurityBlocked {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

impl MemoryEntry {
    pub fn new(content: impl Into<String>) -> Self {
        let now = Self::now_rfc3339();
        Self {
            content: content.into(),
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn now_rfc3339() -> String {
        // Protocol crate is dependency-free; ISO 8601 format without chrono.
        use std::time::{SystemTime, UNIX_EPOCH};
        let dur = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let secs = dur.as_secs();
        let nanos = dur.subsec_nanos();
        // Format as 1970-01-01T00:00:00Z (simplified)
        format!("{}.{:09}Z", secs, nanos)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub memory_enabled: bool,
    pub user_profile_enabled: bool,
    #[serde(default = "default_memory_char_limit")]
    pub memory_char_limit: usize,
    #[serde(default = "default_user_char_limit")]
    pub user_char_limit: usize,
    pub directory: PathBuf,
    #[serde(default = "default_true")]
    pub security_scan: bool,
}

fn default_memory_char_limit() -> usize {
    2200
}

fn default_user_char_limit() -> usize {
    1375
}

fn default_true() -> bool {
    true
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            memory_enabled: true,
            user_profile_enabled: true,
            memory_char_limit: 2200,
            user_char_limit: 1375,
            directory: PathBuf::from(".omnix/memories"),
            security_scan: true,
        }
    }
}
