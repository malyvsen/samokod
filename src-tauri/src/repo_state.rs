// Per-repo samokod state under `.samokod/state.json`: model and effort
// choices, stored by our own role names. Created lazily on first change;
// a missing file means agent defaults.
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::types::ConfigOptionView;

const STATE_PATH: &str = ".samokod/state.json";

/// Stored roles. Field names are ours, never agent-advertised ids.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepoState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
}

/// Our storage role for one agent-advertised option.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigRole {
    Model,
    Effort,
}

/// Classify an agent-advertised option into our roles. Mirrors the
/// frontend's `splitOptions`: model by category, effort by `thought_level`
/// category or `effort` id. One predicate, so a future rename touches here
/// only and stored files keep working.
pub fn classify_option(option: &ConfigOptionView) -> Option<ConfigRole> {
    let category = option.category.as_deref().unwrap_or(&option.id);
    if category == "model" {
        return Some(ConfigRole::Model);
    }
    if category == "thought_level" || option.id == "effort" {
        return Some(ConfigRole::Effort);
    }
    None
}

/// Load the repo file. Missing files yield defaults quietly.
pub fn load_repo_state(repo_root: &Path) -> RepoState {
    let path = repo_root.join(STATE_PATH);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return RepoState::default(),
        Err(error) => {
            log::warn!("failed to read {}: {error}", path.display());
            return RepoState::default();
        }
    };
    if text.trim().is_empty() {
        return RepoState::default();
    }
    match serde_json::from_str(&text) {
        Ok(state) => state,
        Err(error) => {
            log::warn!("failed to parse {}: {error}", path.display());
            RepoState::default()
        }
    }
}

/// Save one role into the repo file, preserving the other.
pub fn save_role(repo_root: &Path, role: ConfigRole, value: &str) {
    let mut state = load_repo_state(repo_root);
    match role {
        ConfigRole::Model => state.model = Some(value.to_string()),
        ConfigRole::Effort => state.effort = Some(value.to_string()),
    }
    let path = repo_root.join(STATE_PATH);
    if let Some(parent) = path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        log::warn!("failed to create {}: {error}", parent.display());
        return;
    }
    match serde_json::to_string_pretty(&state) {
        Ok(text) => {
            if let Err(error) = std::fs::write(&path, text) {
                log::warn!("failed to save {}: {error}", path.display());
            }
        }
        Err(error) => log::warn!("failed to serialize repo state: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(id: &str, category: Option<&str>) -> ConfigOptionView {
        ConfigOptionView {
            id: id.to_string(),
            name: id.to_string(),
            current_value: "v".to_string(),
            options: vec![],
            category: category.map(str::to_string),
        }
    }

    #[test]
    fn classifies_model_and_effort() {
        assert_eq!(
            classify_option(&view("llm", Some("model"))),
            Some(ConfigRole::Model)
        );
        assert_eq!(
            classify_option(&view("x", Some("thought_level"))),
            Some(ConfigRole::Effort)
        );
        assert_eq!(
            classify_option(&view("effort", None)),
            Some(ConfigRole::Effort)
        );
        assert_eq!(classify_option(&view("mode", Some("mode"))), None);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        save_role(dir.path(), ConfigRole::Model, "m1");
        save_role(dir.path(), ConfigRole::Effort, "high");
        assert_eq!(
            load_repo_state(dir.path()),
            RepoState {
                model: Some("m1".to_string()),
                effort: Some("high".to_string()),
            }
        );
    }
}
