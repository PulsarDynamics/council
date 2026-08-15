//! Agent configuration loaded from TOML.
//!
//! Adding a new agent is a TOML-drop, not a code change. See `docs/AGENT_SCHEMA.md`
//! for the full schema. Case-sensitive `name` matching the filename stem is
//! required — see AGENTS.md §5 for the gotcha.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    #[serde(rename = "name")]
    pub name: String,
    pub subscribes: Vec<String>,
    pub publishes: Vec<String>,
    pub model: ModelConfig,
    pub prompt: PromptConfig,
    pub tools: ToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// `openai` | `openrouter` | `ollama` | `azure` | any OpenAI-compatible endpoint.
    pub provider: String,
    pub name: String,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_temperature() -> f32 {
    0.3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub system: String,
    /// Optional template for rendering incoming messages into the LLM prompt.
    #[serde(default)]
    pub template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsConfig {
    /// Names of tools the agent is allowed to invoke. Must be a subset of
    /// tools registered in the `council-agent` binary.
    pub allowed: BTreeSet<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANNER_TOML: &str = r#"
name = "planner"
subscribes = ["goal"]
publishes = ["plan", "broadcast"]

[model]
provider = "openai"
name = "gpt-4o"
temperature = 0.3

[prompt]
system = "You are the Council's Planner."

[tools]
allowed = ["read_file", "ask_user", "delegate_to"]
"#;

    #[test]
    fn parses_minimal_planner_spec() {
        let spec: AgentSpec = toml::from_str(PLANNER_TOML).unwrap();
        assert_eq!(spec.name, "planner");
        assert_eq!(spec.subscribes, vec!["goal"]);
        assert!((spec.model.temperature - 0.3).abs() < f32::EPSILON);
    }
}
