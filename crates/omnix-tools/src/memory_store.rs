use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// A simple flat-file memory store.
/// Entries are stored as plain text lines in a single file.
pub struct MemoryStore {
    path: PathBuf,
    entries: Vec<String>,
}

impl MemoryStore {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Cannot create memory dir: {:?}", parent))?;
            }
            fs::write(&path, "")?;
            return Ok(Self {
                path,
                entries: Vec::new(),
            });
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Cannot read memory file: {:?}", path))?;

        let entries: Vec<String> = content
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(Self { path, entries })
    }

    /// Add a new entry.
    pub fn add(&mut self, content: impl Into<String>) -> Result<()> {
        let content = content.into().trim().to_string();
        if content.is_empty() {
            anyhow::bail!("Cannot add empty entry");
        }
        self.entries.push(content);
        self.save()?;
        Ok(())
    }

    pub fn replace(&mut self, old_text: &str, new_content: impl Into<String>) -> Result<()> {
        let new_content = new_content.into().trim().to_string();
        if new_content.is_empty() {
            anyhow::bail!("Cannot replace with empty content");
        }

        let matches: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.contains(old_text))
            .map(|(idx, _)| idx)
            .collect();

        match matches.len() {
            0 => anyhow::bail!(
                "No memory entry matches the substring '{}'. Current entries:\n{}",
                old_text,
                self.format_entries()
            ),
            1 => {
                let idx = matches[0];
                self.entries[idx] = new_content;
                self.save()?;
                Ok(())
            }
            n => anyhow::bail!(
                "Substring '{}' matches {} entries. Be more specific. Matching entries:\n{}",
                old_text,
                n,
                matches
                    .iter()
                    .map(|&i| format!("  - {}", self.entries[i]))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }

    pub fn remove(&mut self, old_text: &str) -> Result<()> {
        let matches: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.contains(old_text))
            .map(|(idx, _)| idx)
            .collect();

        match matches.len() {
            0 => anyhow::bail!(
                "No memory entry matches the substring '{}'. Current entries:\n{}",
                old_text,
                self.format_entries()
            ),
            1 => {
                let idx = matches[0];
                self.entries.remove(idx);
                self.save()?;
                Ok(())
            }
            n => anyhow::bail!(
                "Substring '{}' matches {} entries. Be more specific. Matching entries:\n{}",
                old_text,
                n,
                matches
                    .iter()
                    .map(|&i| format!("  - {}", self.entries[i]))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        }
    }

    pub fn format_for_prompt(&self) -> String {
        if self.entries.is_empty() {
            return String::new();
        }
        self.entries
            .iter()
            .map(|e| format!("- {}", e))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn save(&self) -> Result<()> {
        let content = self.entries.join("\n");
        if !content.is_empty() {
            let tmp = self.path.with_extension("tmp");
            let mut file = fs::File::create(&tmp)
                .with_context(|| format!("Cannot create temp memory file: {:?}", tmp))?;
            file.write_all(content.as_bytes())
                .with_context(|| format!("Cannot write temp memory file: {:?}", tmp))?;
            file.write_all(b"\n")
                .with_context(|| format!("Cannot write temp memory file: {:?}", tmp))?;
            fs::rename(&tmp, &self.path).with_context(|| {
                format!("Cannot rename memory file: {:?} -> {:?}", tmp, self.path)
            })?;
        } else {
            fs::write(&self.path, "")
                .with_context(|| format!("Cannot write memory file: {:?}", self.path))?;
        }
        Ok(())
    }

    fn format_entries(&self) -> String {
        if self.entries.is_empty() {
            "  (none)".to_string()
        } else {
            self.entries
                .iter()
                .map(|e| format!("  - {}", e))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn memory_store_load_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");
        let store = MemoryStore::load(&path).unwrap();
        assert!(store.entries().is_empty());
        assert!(path.exists());
    }

    #[test]
    fn memory_store_add_and_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        {
            let mut store = MemoryStore::load(&path).unwrap();
            store.add("User prefers dark mode").unwrap();
            store.add("Project is a Rust web service").unwrap();
        }

        let store = MemoryStore::load(&path).unwrap();
        assert_eq!(store.entries().len(), 2);
        assert_eq!(store.entries()[0], "User prefers dark mode");
        assert_eq!(store.entries()[1], "Project is a Rust web service");
    }

    #[test]
    fn memory_store_replace_unique() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("User prefers dark mode").unwrap();
        store.add("Project uses Axum").unwrap();

        store
            .replace("dark mode", "User prefers light mode")
            .unwrap();
        assert_eq!(store.entries()[0], "User prefers light mode");
        assert_eq!(store.entries()[1], "Project uses Axum");
    }

    #[test]
    fn memory_store_replace_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("User prefers dark mode").unwrap();

        let result = store.replace("missing", "x");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("No memory entry matches"));
    }

    #[test]
    fn memory_store_replace_multiple_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("User prefers dark mode").unwrap();
        store.add("User prefers dark themes").unwrap();

        let result = store.replace("dark", "light");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("matches 2 entries"));
    }

    #[test]
    fn memory_store_remove_unique() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("Keep this").unwrap();
        store.add("Remove this").unwrap();

        store.remove("Remove").unwrap();
        assert_eq!(store.entries().len(), 1);
        assert_eq!(store.entries()[0], "Keep this");
    }

    #[test]
    fn memory_store_remove_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("Keep this").unwrap();

        let result = store.remove("missing");
        assert!(result.is_err());
    }

    #[test]
    fn memory_store_remove_multiple_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("User likes coffee").unwrap();
        store.add("User likes tea").unwrap();

        let result = store.remove("likes");
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("matches 2 entries"));
    }

    #[test]
    fn memory_store_format_for_prompt() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        store.add("Entry one").unwrap();
        store.add("Entry two").unwrap();

        let formatted = store.format_for_prompt();
        assert!(formatted.contains("- Entry one"));
        assert!(formatted.contains("- Entry two"));
    }

    #[test]
    fn memory_store_format_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let store = MemoryStore::load(&path).unwrap();
        assert!(store.format_for_prompt().is_empty());
    }

    #[test]
    fn memory_store_add_empty_rejected() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("memory.md");

        let mut store = MemoryStore::load(&path).unwrap();
        let result = store.add("   ");
        assert!(result.is_err());
    }
}
