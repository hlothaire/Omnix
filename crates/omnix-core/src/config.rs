use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level application configuration loaded from `~/.omnix/settings.toml`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub permissions: PermissionConfig,
    #[serde(default)]
    pub session: SessionConfig,
    #[serde(default)]
    pub compaction: CompactionConfig,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

impl AppConfig {
    /// Load configuration from `~/.omnix/settings.toml`.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            let default = Self::default();
            default.save()?;
            return Ok(default);
        }

        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file {:?}", path))?;

        let config: AppConfig = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file {:?}", path))?;

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }

        let contents =
            toml::to_string_pretty(self).with_context(|| "Failed to serialize config to TOML")?;

        std::fs::write(&path, contents)
            .with_context(|| format!("Failed to write config file {:?}", path))?;

        Ok(())
    }

    pub fn config_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("Failed to determine home directory")?;
        Ok(home.join(".omnix").join("settings.toml"))
    }

    pub fn expand_home(path: &str) -> Result<PathBuf> {
        if let Some(stripped) = path.strip_prefix("~/") {
            let home = dirs::home_dir().context("Failed to determine home directory")?;
            Ok(home.join(stripped))
        } else {
            Ok(PathBuf::from(path))
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    #[serde(default = "default_provider_kind")]
    pub kind: String,
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default)]
    pub model: String,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: default_provider_kind(),
            host: default_host(),
            model: String::new(),
        }
    }
}

fn default_provider_kind() -> String {
    String::new()
}

fn default_host() -> String {
    String::new()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PermissionConfig {
    #[serde(default = "default_permission_mode")]
    pub mode: String,
}

impl Default for PermissionConfig {
    fn default() -> Self {
        Self {
            mode: default_permission_mode(),
        }
    }
}

fn default_permission_mode() -> String {
    "workspace-write".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SessionConfig {
    #[serde(default = "default_session_dir")]
    pub directory: String,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            directory: default_session_dir(),
        }
    }
}

fn default_session_dir() -> String {
    "~/.omnix/sessions".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CompactionConfig {
    #[serde(default = "default_requested_output_tokens")]
    pub requested_output_tokens: u32,
    #[serde(default = "default_min_safety_margin_tokens")]
    pub min_safety_margin_tokens: usize,
    #[serde(default = "default_safety_margin_percent")]
    pub safety_margin_percent: u8,
    #[serde(default = "default_max_keep_recent_tokens")]
    pub max_keep_recent_tokens: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            requested_output_tokens: default_requested_output_tokens(),
            min_safety_margin_tokens: default_min_safety_margin_tokens(),
            safety_margin_percent: default_safety_margin_percent(),
            max_keep_recent_tokens: default_max_keep_recent_tokens(),
        }
    }
}

fn default_requested_output_tokens() -> u32 {
    4096
}

fn default_min_safety_margin_tokens() -> usize {
    512
}

fn default_safety_margin_percent() -> u8 {
    5
}

fn default_max_keep_recent_tokens() -> usize {
    20_000
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_telemetry_enabled")]
    pub enabled: bool,
    #[serde(default = "default_telemetry_path")]
    pub path: String,
    #[serde(default = "default_telemetry_max_size_mb")]
    pub max_file_size_mb: u64,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: default_telemetry_enabled(),
            path: default_telemetry_path(),
            max_file_size_mb: default_telemetry_max_size_mb(),
        }
    }
}

fn default_telemetry_enabled() -> bool {
    false
}

fn default_telemetry_path() -> String {
    "~/.omnix/telemetry.jsonl".to_string()
}

fn default_telemetry_max_size_mb() -> u64 {
    10
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_default_values() {
        let config = AppConfig::default();
        assert_eq!(config.provider.kind, "");
        assert_eq!(config.provider.host, "");
        assert_eq!(config.provider.model, "");
        assert_eq!(config.permissions.mode, "workspace-write");
        assert_eq!(config.session.directory, "~/.omnix/sessions");
        assert_eq!(config.compaction.requested_output_tokens, 4096);
        assert_eq!(config.compaction.min_safety_margin_tokens, 512);
        assert_eq!(config.compaction.safety_margin_percent, 5);
        assert_eq!(config.compaction.max_keep_recent_tokens, 20_000);
        assert!(!config.telemetry.enabled);
        assert_eq!(config.telemetry.path, "~/.omnix/telemetry.jsonl");
        assert_eq!(config.telemetry.max_file_size_mb, 10);
    }

    #[test]
    fn test_config_parse_toml() {
        let toml = r#"
[provider]
kind = "ollama"
host = "http://localhost:11434"
model = "qwen2.5:0.5b"

[permissions]
mode = "readonly"

[session]
directory = "~/custom/sessions"

[compaction]
requested_output_tokens = 2048
min_safety_margin_tokens = 256
safety_margin_percent = 10
max_keep_recent_tokens = 12000

[telemetry]
enabled = true
path = "~/custom/telemetry.jsonl"
max_file_size_mb = 5
"#;

        let config: AppConfig = toml::from_str(toml).unwrap();
        assert_eq!(config.provider.kind, "ollama");
        assert_eq!(config.provider.host, "http://localhost:11434");
        assert_eq!(config.provider.model, "qwen2.5:0.5b");
        assert_eq!(config.permissions.mode, "readonly");
        assert_eq!(config.session.directory, "~/custom/sessions");
        assert_eq!(config.compaction.requested_output_tokens, 2048);
        assert_eq!(config.compaction.min_safety_margin_tokens, 256);
        assert_eq!(config.compaction.safety_margin_percent, 10);
        assert_eq!(config.compaction.max_keep_recent_tokens, 12_000);
        assert!(config.telemetry.enabled);
        assert_eq!(config.telemetry.path, "~/custom/telemetry.jsonl");
        assert_eq!(config.telemetry.max_file_size_mb, 5);
    }

    #[test]
    fn test_config_parse_partial_toml() {
        let toml = r#"
[provider]
model = "llama3.1:8b"
"#;

        let config: AppConfig = toml::from_str(toml).unwrap();
        // Provider: model overridden, rest defaults
        assert_eq!(config.provider.kind, "");
        assert_eq!(config.provider.host, "");
        assert_eq!(config.provider.model, "llama3.1:8b");
        // Rest: all defaults
        assert_eq!(config.permissions.mode, "workspace-write");
        assert_eq!(config.compaction.requested_output_tokens, 4096);
    }

    #[test]
    fn test_expand_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(AppConfig::expand_home("~/test").unwrap(), home.join("test"));
        assert_eq!(
            AppConfig::expand_home("/absolute/path").unwrap(),
            PathBuf::from("/absolute/path")
        );
    }

    #[test]
    fn test_config_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let config_path = tmp.path().join("settings.toml");

        let config = AppConfig {
            provider: ProviderConfig {
                kind: "ollama".to_string(),
                host: "http://localhost:11434".to_string(),
                model: "test-model".to_string(),
            },
            ..AppConfig::default()
        };

        let contents = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&config_path, contents).unwrap();

        let loaded_contents = std::fs::read_to_string(&config_path).unwrap();
        let loaded: AppConfig = toml::from_str(&loaded_contents).unwrap();

        assert_eq!(loaded.provider.kind, "ollama");
        assert_eq!(loaded.provider.model, "test-model");
        assert_eq!(loaded.permissions.mode, "workspace-write");
    }

    #[test]
    fn test_config_serialize_roundtrip() {
        let config = AppConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.provider.kind, parsed.provider.kind);
        assert_eq!(config.provider.host, parsed.provider.host);
        assert_eq!(config.provider.model, parsed.provider.model);
        assert_eq!(config.permissions.mode, parsed.permissions.mode);
        assert_eq!(config.session.directory, parsed.session.directory);
        assert_eq!(
            config.compaction.requested_output_tokens,
            parsed.compaction.requested_output_tokens
        );
        assert_eq!(
            config.compaction.min_safety_margin_tokens,
            parsed.compaction.min_safety_margin_tokens
        );
        assert_eq!(
            config.compaction.safety_margin_percent,
            parsed.compaction.safety_margin_percent
        );
        assert_eq!(
            config.compaction.max_keep_recent_tokens,
            parsed.compaction.max_keep_recent_tokens
        );
        assert_eq!(config.telemetry.enabled, parsed.telemetry.enabled);
        assert_eq!(config.telemetry.path, parsed.telemetry.path);
        assert_eq!(
            config.telemetry.max_file_size_mb,
            parsed.telemetry.max_file_size_mb
        );
    }
}
