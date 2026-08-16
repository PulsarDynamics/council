//! Shared provider config on disk. Both the orchestrator and the agent
//! processes read `providers.toml` to discover custom providers. Built-ins
//! (OpenAI Chat, OpenAI Responses, Anthropic Messages) are always
//! available; entries in this file can override their defaults or
//! register new ones.
//!
//! File format (TOML):
//!
//! ```toml
//! [providers.groq]
//! kind = "openai_chat"
//! base_url = "https://api.groq.com/openai/v1"
//! api_key = "gsk_..."
//! default_model = "llama-3.3-70b-versatile"
//!
//! [providers.local-llama]
//! kind = "openai_chat"
//! base_url = "http://localhost:11434/v1"
//! api_key = ""
//! default_model = "llama3.2"
//! ```
//!
//! Default path: `$COUNCIL_PROVIDERS_FILE` or `~/.config/council/providers.toml`.
//! The orchestrator creates the parent dir on first write.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Wire format the provider speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    /// OpenAI /v1/chat/completions. Also works with any OpenAI-compatible
    /// endpoint that follows the chat/completions shape (Together, Groq,
    /// OpenRouter's compat mode, local llama.cpp, etc.).
    #[serde(rename = "openai_chat")]
    OpenAiChat,
    /// OpenAI /v1/responses. Newer API with first-class tool/agent support.
    #[serde(rename = "openai_responses")]
    OpenAiResponses,
    /// Anthropic /v1/messages. Distinctive tool-use + content-block shape.
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
    /// Any other OpenAI-compatible /v1/chat/completions endpoint. Same
    /// wire format as `OpenAiChat` but conceptually a "custom" entry the
    /// user added in the settings menu.
    #[serde(rename = "custom")]
    Custom,
}

impl ProviderKind {
    pub fn label(&self) -> &'static str {
        match self {
            Self::OpenAiChat => "OpenAI Chat Completions",
            Self::OpenAiResponses => "OpenAI Responses",
            Self::AnthropicMessages => "Anthropic Messages",
            Self::Custom => "Custom",
        }
    }
}

/// Flat provider config the agent loop and registry use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
}

/// On-disk representation: `[providers.<name>]` tables.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProvidersFile {
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderEntry {
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
}

impl ProvidersFile {
    /// Load a providers file. Missing file or parse error → empty (caller
    /// can choose to log).
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Write the file atomically (write to .tmp, then rename) so a half-
    /// written file never breaks the agent.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Flatten into the `Vec<ProviderConfig>` the registry expects.
    pub fn flatten(&self) -> Vec<ProviderConfig> {
        self.providers
            .iter()
            .map(|(name, e)| ProviderConfig {
                name: name.clone(),
                kind: e.kind,
                base_url: e.base_url.clone(),
                api_key: e.api_key.clone(),
                default_model: e.default_model.clone(),
            })
            .collect()
    }

    /// Upsert a provider.
    pub fn upsert(&mut self, name: &str, entry: ProviderEntry) {
        self.providers.insert(name.to_string(), entry);
    }

    /// Remove a provider by name. Returns true if it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        self.providers.remove(name).is_some()
    }
}

/// Default providers-file path. Override with `COUNCIL_PROVIDERS_FILE`.
pub fn default_providers_path() -> PathBuf {
    if let Ok(p) = std::env::var("COUNCIL_PROVIDERS_FILE") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/council/providers.toml");
    }
    PathBuf::from("./providers.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let mut f = ProvidersFile::default();
        f.upsert(
            "groq",
            ProviderEntry {
                kind: ProviderKind::OpenAiChat,
                base_url: "https://api.groq.com/openai/v1".into(),
                api_key: "gsk_x".into(),
                default_model: "llama-3.3-70b-versatile".into(),
            },
        );
        let s = toml::to_string(&f).unwrap();
        let back: ProvidersFile = toml::from_str(&s).unwrap();
        assert_eq!(back.providers.len(), 1);
        assert!(back.providers.contains_key("groq"));
    }

    #[test]
    fn missing_file_returns_empty() {
        let f = ProvidersFile::load(Path::new("/nonexistent/path/providers.toml"));
        assert!(f.providers.is_empty());
    }
}
