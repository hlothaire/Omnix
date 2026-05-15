use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use tokio::sync::mpsc;

/// A single telemetry record written as one JSONL line.
#[derive(Debug, Serialize)]
struct TelemetryRecord {
    /// ISO 8601 timestamp.
    ts: String,
    /// Session identifier.
    session_id: String,
    /// Event category.
    category: &'static str,
    /// Event name.
    event: &'static str,
    /// Arbitrary payload (trimmed to avoid huge lines).
    payload: serde_json::Value,
}

/// Telemetry sink configuration.
#[derive(Clone)]
pub struct TelemetrySink {
    /// Channel sender for the background writer.
    tx: mpsc::UnboundedSender<TelemetryRecord>,
}

impl TelemetrySink {
    /// Create and spawn a new telemetry sink.
    ///
    /// * `path` — JSONL file path (home expansion applied by caller).
    /// * `max_size_bytes` — Rotate file when it exceeds this size.
    pub fn new(path: PathBuf, max_size_bytes: u64) -> Self {
        let (tx, mut rx) = mpsc::unbounded_channel::<TelemetryRecord>();

        tokio::spawn(async move {
            let mut writer = match LogWriter::new(&path, max_size_bytes) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("[telemetry] Failed to initialize: {}", e);
                    return;
                }
            };

            // Flush every 5 seconds or when channel closes
            let mut flush_interval = tokio::time::interval(Duration::from_secs(5));

            loop {
                tokio::select! {
                    Some(record) = rx.recv() => {
                        if let Err(e) = writer.write_record(&record) {
                            eprintln!("[telemetry] Write error: {}", e);
                        }
                    }
                    _ = flush_interval.tick() => {
                        if let Err(e) = writer.flush() {
                            eprintln!("[telemetry] Flush error: {}", e);
                        }
                    }
                    else => break,
                }
            }

            let _ = writer.flush();
        });

        Self { tx }
    }

    /// Log a prompt sent by the user.
    pub fn log_prompt(&self, session_id: &str, text: &str) {
        self.send(
            session_id,
            "interaction",
            "prompt",
            serde_json::json!({
                "text_preview": truncate(text, 500),
                "text_length": text.len(),
            }),
        );
    }

    /// Log a tool call invocation.
    pub fn log_tool_call(&self, session_id: &str, tool_name: &str, input: &serde_json::Value) {
        self.send(
            session_id,
            "tool",
            "call",
            serde_json::json!({
                "tool": tool_name,
                "input_preview": truncate(&input.to_string(), 500),
            }),
        );
    }

    /// Log a tool call result.
    pub fn log_tool_result(
        &self,
        session_id: &str,
        tool_name: &str,
        output: &str,
        is_error: bool,
        duration_ms: u64,
    ) {
        self.send(
            session_id,
            "tool",
            "result",
            serde_json::json!({
                "tool": tool_name,
                "is_error": is_error,
                "output_preview": truncate(output, 500),
                "duration_ms": duration_ms,
            }),
        );
    }

    /// Log token usage for a turn.
    pub fn log_usage(&self, session_id: &str, input_tokens: u64, output_tokens: u64) {
        self.send(
            session_id,
            "usage",
            "tokens",
            serde_json::json!({
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
            }),
        );
    }

    /// Log an error.
    pub fn log_error(&self, session_id: &str, message: &str, retryable: bool) {
        self.send(
            session_id,
            "error",
            if retryable { "retryable" } else { "fatal" },
            serde_json::json!({
                "message": truncate(message, 1000),
            }),
        );
    }

    /// Log a compaction event.
    pub fn log_compaction(&self, session_id: &str, removed_count: usize, summary_preview: &str) {
        self.send(
            session_id,
            "session",
            "compaction",
            serde_json::json!({
                "removed_count": removed_count,
                "summary_preview": truncate(summary_preview, 500),
            }),
        );
    }

    fn send(
        &self,
        session_id: &str,
        category: &'static str,
        event: &'static str,
        payload: serde_json::Value,
    ) {
        let record = TelemetryRecord {
            ts: Utc::now().to_rfc3339(),
            session_id: session_id.to_string(),
            category,
            event,
            payload,
        };
        let _ = self.tx.send(record);
    }
}

/// Manages file writing with rotation.
struct LogWriter {
    path: PathBuf,
    max_size: u64,
    writer: BufWriter<File>,
    current_size: u64,
}

impl LogWriter {
    fn new(path: &Path, max_size: u64) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create telemetry directory {:?}", parent))?;
        }

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("Failed to open telemetry file {:?}", path))?;

        let current_size = file.metadata()?.len();
        let writer = BufWriter::new(file);

        Ok(Self {
            path: path.to_path_buf(),
            max_size,
            writer,
            current_size,
        })
    }

    fn write_record(&mut self, record: &TelemetryRecord) -> Result<()> {
        let line = serde_json::to_string(record)?;
        let line_bytes = line.len() as u64 + 1; // +1 for newline

        if self.current_size + line_bytes > self.max_size {
            self.rotate()?;
        }

        self.writer.write_all(line.as_bytes())?;
        self.writer.write_all(b"\n")?;
        self.current_size += line_bytes;

        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    fn rotate(&mut self) -> Result<()> {
        self.writer.flush()?;
        // Old writer will be dropped when replaced below, closing the file handle

        // Move current file to .1, .1 to .2, etc.
        for i in (1..=4).rev() {
            let old = self.path.with_extension(format!("jsonl.{}", i));
            let new = self.path.with_extension(format!("jsonl.{}", i + 1));
            if old.exists() {
                let _ = fs::rename(&old, &new);
            }
        }

        let backup = self.path.with_extension("jsonl.1");
        let _ = fs::rename(&self.path, &backup);

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("Failed to open new telemetry file {:?}", self.path))?;

        self.writer = BufWriter::new(file);
        self.current_size = 0;

        Ok(())
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...[truncated, {} total chars]", &s[..max_len], s.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        let long = "a".repeat(1000);
        let truncated = truncate(&long, 100);
        assert!(truncated.contains("[truncated"));
        assert!(truncated.len() < 200);
    }

    #[tokio::test]
    async fn test_telemetry_sink_writes() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("telemetry.jsonl");
        let sink = TelemetrySink::new(path.clone(), 1024 * 1024);

        sink.log_prompt("test-session", "Hello world");
        sink.log_tool_call(
            "test-session",
            "bash",
            &serde_json::json!({"command": "ls"}),
        );
        sink.log_tool_result("test-session", "bash", "file.txt\nfile2.txt", false, 150);
        sink.log_usage("test-session", 100, 50);
        sink.log_error("test-session", "Something went wrong", false);
        sink.log_compaction("test-session", 10, "Summary of old conversation");

        // Give writer task time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"event\":\"prompt\""));
        assert!(contents.contains("\"event\":\"call\""));
        assert!(contents.contains("\"event\":\"result\""));
        assert!(contents.contains("\"event\":\"tokens\""));
        assert!(contents.contains("\"event\":\"fatal\""));
        assert!(contents.contains("\"event\":\"compaction\""));
        assert!(contents.contains("\"session_id\":\"test-session\""));
    }

    #[tokio::test]
    async fn test_telemetry_rotation() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("telemetry.jsonl");

        // Very small max size to force rotation
        let sink = TelemetrySink::new(path.clone(), 100);

        // Write enough to trigger rotation
        for i in 0..20 {
            sink.log_prompt(
                "test-session",
                &format!("Message number {} with some padding", i),
            );
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Original should exist and backup should be created
        assert!(path.exists());
        let backup = path.with_extension("jsonl.1");
        assert!(backup.exists());
    }
}
