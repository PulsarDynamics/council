//! Provider registry. Built-ins (OpenAI Chat, OpenAI Responses, Anthropic)
//! are always available. Custom providers (added via the UI settings menu)
//! can be layered on top — they're keyed by name and override the built-in
//! of the same name (or add a new one if the name is new).

use std::collections::HashMap;
use std::sync::Arc;

use super::providers::{AnthropicProvider, OpenAiChatProvider, OpenAiResponsesProvider};
use super::{LlmProvider, ProviderConfig, ProviderKind};

/// Look up which env var holds a given provider's API key.
pub fn api_key_env_for(name: &str) -> String {
    format!("COUNCIL_PROVIDER_{}_API_KEY", name.to_uppercase())
}

/// Read the provider config for `name` from env, returning a built-in if no
/// override is set. Built-in providers are always present; users can
/// override their `base_url` and `default_model` by setting
/// `COUNCIL_PROVIDER_<NAME>_BASE_URL` / `_MODEL` env vars (the API key is
/// read from `COUNCIL_PROVIDER_<NAME>_API_KEY` if set, falling back to
/// `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` for the built-ins).
pub fn load_config(name: &str) -> Option<ProviderConfig> {
    let kind = match name {
        "openai" | "openai-chat" => Some(ProviderKind::OpenAiChat),
        "openai-responses" => Some(ProviderKind::OpenAiResponses),
        "anthropic" => Some(ProviderKind::AnthropicMessages),
        _ => None,
    };

    // Custom provider loaded from env alone.
    let kind = match std::env::var(format!("COUNCIL_PROVIDER_{}_KIND", name.to_uppercase())) {
        Ok(s) => match s.as_str() {
            "openai_chat" | "openai-chat" => Some(ProviderKind::OpenAiChat),
            "openai_responses" | "openai-responses" => Some(ProviderKind::OpenAiResponses),
            "anthropic_messages" | "anthropic" => Some(ProviderKind::AnthropicMessages),
            "custom" => Some(ProviderKind::Custom),
            _ => kind,
        },
        Err(_) => kind,
    };

    let kind = kind?;

    // Base URL: env override, then built-in default.
    let base_url = std::env::var(format!("COUNCIL_PROVIDER_{}_BASE_URL", name.to_uppercase()))
        .ok()
        .or_else(|| {
            std::env::var("OPENAI_BASE_URL").ok().filter(|_| matches!(kind, ProviderKind::OpenAiChat | ProviderKind::OpenAiResponses))
        })
        .unwrap_or_else(|| default_base_url(kind));

    // API key: provider-specific env, then well-known env, then error.
    let api_key = std::env::var(format!("COUNCIL_PROVIDER_{}_API_KEY", name.to_uppercase()))
        .ok()
        .or_else(|| match kind {
            ProviderKind::OpenAiChat | ProviderKind::OpenAiResponses => {
                std::env::var("OPENAI_API_KEY").ok()
            }
            ProviderKind::AnthropicMessages => std::env::var("ANTHROPIC_API_KEY").ok(),
            ProviderKind::Custom => None,
        })
        .unwrap_or_default();

    // Default model: env override, then built-in default.
    let default_model = std::env::var(format!("COUNCIL_PROVIDER_{}_MODEL", name.to_uppercase()))
        .ok()
        .unwrap_or_else(|| default_model(kind));

    Some(ProviderConfig {
        name: name.to_string(),
        kind,
        base_url,
        api_key,
        default_model,
    })
}

fn default_base_url(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::OpenAiChat | ProviderKind::OpenAiResponses => {
            "https://api.openai.com/v1".into()
        }
        ProviderKind::AnthropicMessages => "https://api.anthropic.com/v1".into(),
        ProviderKind::Custom => String::new(),
    }
}

fn default_model(kind: ProviderKind) -> String {
    match kind {
        ProviderKind::OpenAiChat | ProviderKind::OpenAiResponses => "gpt-4o".into(),
        ProviderKind::AnthropicMessages => "claude-sonnet-4-5".into(),
        ProviderKind::Custom => String::new(),
    }
}

/// Holds the resolved LLM providers, keyed by name. Lookups happen by
/// the agent's TOML `provider` field.
#[derive(Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    /// Build a registry with the three built-in providers.
    pub fn new() -> Self {
        let mut providers: HashMap<String, Arc<dyn LlmProvider>> = HashMap::new();
        providers.insert("openai".into(), Arc::new(OpenAiChatProvider::new()));
        providers.insert("openai-chat".into(), Arc::new(OpenAiChatProvider::new()));
        providers.insert("openai-responses".into(), Arc::new(OpenAiResponsesProvider::new()));
        providers.insert("anthropic".into(), Arc::new(AnthropicProvider::new()));
        Self { providers }
    }

    /// Register or replace a provider. If `name` is one of the built-ins,
    /// the custom one wins. The provider's `default_base_url` is used as a
    /// hint; if the loaded `ProviderConfig` differs, the caller should pass
    /// the config separately (we don't change the impl per call).
    pub fn register(&mut self, provider: Arc<dyn LlmProvider>) {
        self.providers.insert(provider.name().to_string(), provider);
    }

    /// Look up by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers.get(name).cloned()
    }

    /// List all known provider names.
    pub fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.providers.keys().cloned().collect();
        v.sort();
        v
    }
}
