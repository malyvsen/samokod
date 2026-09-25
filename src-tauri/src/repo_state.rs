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

impl ConfigRole {
    /// Both roles, model first: effort options can depend on the model.
    pub const ALL: [ConfigRole; 2] = [ConfigRole::Model, ConfigRole::Effort];

    /// Role name for logs. Matches the `RepoState` field names.
    pub fn name(self) -> &'static str {
        match self {
            ConfigRole::Model => "model",
            ConfigRole::Effort => "effort",
        }
    }

    /// Current value for this role. Pure.
    pub fn get(self, roles: &RepoState) -> Option<&String> {
        match self {
            ConfigRole::Model => roles.model.as_ref(),
            ConfigRole::Effort => roles.effort.as_ref(),
        }
    }

    /// Write this role, preserving the other. Pure.
    pub fn set(self, roles: &mut RepoState, value: Option<String>) {
        match self {
            ConfigRole::Model => roles.model = value,
            ConfigRole::Effort => roles.effort = value,
        }
    }
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

/// Current model/effort values by role. Absent roles stay `None`. Pure.
pub fn roles_from_options(options: &[ConfigOptionView]) -> RepoState {
    let mut roles = RepoState::default();
    for option in options {
        match classify_option(option) {
            Some(role) if role.get(&roles).is_none() => {
                role.set(&mut roles, Some(option.current_value.clone()));
            }
            _ => {}
        }
    }
    roles
}

/// Write one role as set or cleared, preserving the other key.
pub fn set_role(repo_root: &Path, role: ConfigRole, value: Option<&str>) {
    let mut state = load_repo_state(repo_root);
    role.set(&mut state, value.map(str::to_string));
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

    fn view(id: &str, category: Option<&str>, value: &str) -> ConfigOptionView {
        ConfigOptionView {
            id: id.to_string(),
            name: id.to_string(),
            current_value: value.to_string(),
            options: vec![],
            category: category.map(str::to_string),
        }
    }

    #[test]
    fn classifies_model_and_effort() {
        assert_eq!(
            classify_option(&view("llm", Some("model"), "v")),
            Some(ConfigRole::Model)
        );
        assert_eq!(
            classify_option(&view("x", Some("thought_level"), "v")),
            Some(ConfigRole::Effort)
        );
        assert_eq!(
            classify_option(&view("effort", None, "v")),
            Some(ConfigRole::Effort)
        );
        assert_eq!(classify_option(&view("mode", Some("mode"), "v")), None);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        set_role(dir.path(), ConfigRole::Model, Some("m1"));
        set_role(dir.path(), ConfigRole::Effort, Some("high"));
        assert_eq!(
            load_repo_state(dir.path()),
            RepoState {
                model: Some("m1".to_string()),
                effort: Some("high".to_string()),
            }
        );
    }

    #[test]
    fn roles_from_both_present() {
        let options = vec![
            view("llm", Some("model"), "m1"),
            view("effort", Some("thought_level"), "high"),
        ];
        assert_eq!(
            roles_from_options(&options),
            RepoState {
                model: Some("m1".to_string()),
                effort: Some("high".to_string()),
            }
        );
    }

    #[test]
    fn roles_absent_when_effort_missing() {
        let options = vec![view("llm", Some("model"), "m1")];
        assert_eq!(
            roles_from_options(&options),
            RepoState {
                model: Some("m1".to_string()),
                effort: None,
            }
        );
    }

    #[test]
    fn roles_ignore_mode_and_extras() {
        let options = vec![
            view("mode", Some("mode"), "planner"),
            view("ctx", Some("model_config"), "big"),
            view("llm", Some("model"), "m1"),
        ];
        assert_eq!(
            roles_from_options(&options),
            RepoState {
                model: Some("m1".to_string()),
                effort: None,
            }
        );
    }

    #[test]
    fn clear_preserves_other_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        set_role(dir.path(), ConfigRole::Model, Some("m1"));
        set_role(dir.path(), ConfigRole::Effort, Some("xhigh"));
        set_role(dir.path(), ConfigRole::Effort, None);
        assert_eq!(
            load_repo_state(dir.path()),
            RepoState {
                model: Some("m1".to_string()),
                effort: None,
            }
        );
    }
}
