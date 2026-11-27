// Session config: reading and reapplying model/effort options per
// session, with repo-wide and global defaults. All model/effort
// lifecycle lives here: classifying options, reapplying defaults on
// spawn, applying global changes, and persisting moved roles.
use crate::acp::{
    self, Agent, ConnectionTo, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions,
};
use crate::repo_state::{ConfigRole, RepoState};
use crate::types::{AgentError, ConfigOptionValueView, ConfigOptionView, RepoDefaults, SessionKey};

use super::AgentManager;

impl AgentManager {
    /// Set one config option repo-wide: every live session gets the same
    /// value, each `last_roles` refreshes from its own response, and moved
    /// model/effort roles persist as the repo default and the global
    /// default. Returns the last session's option list for the settings
    /// paint; with no live sessions it fails instead of guessing.
    pub async fn set_global_config_option(
        &self,
        window: &str,
        config_id: String,
        value: String,
    ) -> Result<Vec<ConfigOptionView>, AgentError> {
        let live: Vec<(SessionKey, ConnectionTo<Agent>, String)> = match self.state.lock() {
            Ok(backend) => backend
                .windows
                .get(window)
                .map(|win| {
                    win.sessions
                        .iter()
                        .filter_map(|(key, live)| {
                            let connection = live.connection.clone()?;
                            let session_id = live.session_id.clone()?;
                            (!connection.is_incoming_closed())
                                .then(|| (key.clone(), connection, session_id))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Err(error) => {
                log::warn!("failed to snapshot live sessions: {error}");
                Vec::new()
            }
        };
        if live.is_empty() {
            return Err(AgentError::NoSession {
                raw: "no live sessions to configure".to_string(),
            });
        }
        let mut last = Vec::new();
        for (key, connection, session_id) in live {
            let response = send_config_option(
                &connection,
                &acp::SessionId::new(session_id),
                &config_id,
                &value,
            )
            .await
            .map_err(|error| AgentError::RequestFailed { raw: error })?;
            let roles = crate::repo_state::roles_from_options(&response);
            let moved = match self.state.lock() {
                Ok(mut backend) => match backend.windows.get_mut(window) {
                    Some(win) => {
                        let current = win
                            .sessions
                            .get(&key)
                            .map(|live| live.last_roles.clone())
                            .unwrap_or_default();
                        let moved: Vec<ConfigRole> = ConfigRole::ALL
                            .into_iter()
                            .filter(|role| role.get(&current) != role.get(&roles))
                            .collect();
                        if let Some(live) = win.sessions.get_mut(&key) {
                            live.last_roles = roles.clone();
                        }
                        Some(moved)
                    }
                    None => None,
                },
                Err(error) => {
                    log::warn!("failed to remember global choice: {error}");
                    None
                }
            };
            if let Some(moved) = moved {
                self.persist_moved_roles(window, &moved, &roles);
            }
            log_roles("selected", &roles);
            last = response;
        }
        Ok(last)
    }

    /// Effective model/effort defaults for one repo: the per-repo file
    /// wins, else the last manually selected global values.
    pub(crate) fn config_defaults(&self, repo_root: &std::path::Path) -> RepoDefaults {
        let stored = crate::repo_state::load_repo_state(repo_root);
        crate::repo_state::effective_defaults(&stored, &self.app_prefs())
    }

    /// Persist moved model/effort roles per-repo and globally. The repo
    /// write is skipped without a current repo; the global write always
    /// runs so a new repo inherits the last manual choice. Prefs failures
    /// only warn, never failing the config change.
    pub(crate) fn persist_moved_roles(
        &self,
        window: &str,
        moved: &[ConfigRole],
        roles: &RepoState,
    ) {
        if moved.is_empty() {
            return;
        }
        if let Some(repo) = self.current_repo(window) {
            for role in moved {
                crate::repo_state::set_role(&repo, *role, role.get(roles).map(String::as_str));
            }
        }
        let Some(dir) = self.prefs_dir() else {
            return;
        };
        let mut prefs = crate::prefs::load_prefs(&dir);
        for role in moved {
            crate::prefs::record_global_choice(
                &mut prefs,
                *role,
                role.get(roles).map(String::as_str),
            );
        }
        if let Err(error) = crate::prefs::save_prefs(&dir, &prefs) {
            log::warn!("failed to save global defaults: {error}");
        }
    }
}

/// Reapply the effective default model then effort on one fresh session
/// (effort-last, since a model switch can reshape effort options). Each
/// absent or inapplicable role is skipped with a warn-log while the open
/// continues on defaults. Returns the refreshed option list and the
/// applied roles for `last_roles`. Spawn and global change share this
/// round-trip shape with `send_config_option`; one log shape via
/// `log_roles`.
pub(crate) async fn reapply_defaults(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
    options: Vec<ConfigOptionView>,
    effective: &RepoDefaults,
) -> (Vec<ConfigOptionView>, RepoState) {
    // Per-repo choices win, otherwise the global defaults apply.
    let mut options = options;
    let mut applied = RepoState::default();
    for role in ConfigRole::ALL {
        let Some(wanted) = role.get_default(effective).cloned() else {
            continue;
        };
        let Some(id) = option_id_for_role(&options, role) else {
            log::warn!("default {} has no matching option, skipping", role.name());
            continue;
        };
        match send_config_option(connection, session_id, &id, &wanted).await {
            Ok(updated) => {
                options = updated;
                role.set(&mut applied, Some(wanted));
            }
            Err(error) => {
                log::warn!(
                    "failed to reapply default {} {wanted}: {error}",
                    role.name()
                )
            }
        }
    }
    log_roles("applied", &applied);
    (options, applied)
}

/// Select one agent by mode id. Fails loud: a scoping session running as
/// the wrong agent would silently break write confinement.
pub(crate) async fn pin_agent_mode(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
    mode: &str,
) -> Result<Vec<ConfigOptionView>, AgentError> {
    send_config_option(connection, session_id, "mode", mode)
        .await
        .map_err(|error| AgentError::RequestFailed {
            raw: format!("failed to select agent {mode}: {error}"),
        })
}

/// Agent-advertised option id currently filling a storage role.
pub(crate) fn option_id_for_role(options: &[ConfigOptionView], role: ConfigRole) -> Option<String> {
    options
        .iter()
        .find(|option| crate::repo_state::classify_option(option) == Some(role))
        .map(|option| option.id.clone())
}

/// Log value for a role, marking cleared roles. Pure.
fn role_label(value: Option<&str>) -> &str {
    value.unwrap_or("<cleared>")
}

/// One info-log for both roles after a select or reapply. Pure formatting,
/// logging edge.
pub(crate) fn log_roles(action: &str, roles: &RepoState) {
    log::info!(
        "{action} model={} effort={}",
        role_label(roles.model.as_deref()),
        role_label(roles.effort.as_deref())
    );
}

/// One `session/set_config_option` round trip. Callers decide how loud
/// the failure is.
pub(crate) async fn send_config_option(
    connection: &ConnectionTo<Agent>,
    session_id: &acp::SessionId,
    config_id: &str,
    value: &str,
) -> Result<Vec<ConfigOptionView>, String> {
    connection
        .send_request(acp::build_set_config_request(session_id, config_id, value))
        .block_task()
        .await
        .map(|response| config_views(&response.config_options))
        .map_err(|error| error.to_string())
}

/// Convert wire session config options to views.
pub fn config_views(options: &[SessionConfigOption]) -> Vec<ConfigOptionView> {
    options
        .iter()
        .map(|option| {
            let (current_value, values) = match &option.kind {
                SessionConfigKind::Select(select) => (
                    select.current_value.to_string(),
                    select_values(&select.options),
                ),
                SessionConfigKind::Boolean(boolean) => (
                    boolean.current_value.to_string(),
                    vec![
                        ConfigOptionValueView {
                            value: "true".to_string(),
                            name: "true".to_string(),
                        },
                        ConfigOptionValueView {
                            value: "false".to_string(),
                            name: "false".to_string(),
                        },
                    ],
                ),
                _ => (String::new(), Vec::new()),
            };
            ConfigOptionView {
                id: option.id.to_string(),
                name: option.name.clone(),
                current_value,
                options: values,
                category: option_category(&option.category),
            }
        })
        .collect()
}

/// Spec `category` as a plain string, absent when the agent omits it.
fn option_category(category: &Option<SessionConfigOptionCategory>) -> Option<String> {
    match category {
        None => None,
        Some(SessionConfigOptionCategory::Mode) => Some("mode".to_string()),
        Some(SessionConfigOptionCategory::Model) => Some("model".to_string()),
        Some(SessionConfigOptionCategory::ModelConfig) => Some("model_config".to_string()),
        Some(SessionConfigOptionCategory::ThoughtLevel) => Some("thought_level".to_string()),
        Some(SessionConfigOptionCategory::Other(name)) => Some(name.clone()),
        // Future SDK variants surface as absent.
        Some(_) => None,
    }
}

fn select_values(options: &SessionConfigSelectOptions) -> Vec<ConfigOptionValueView> {
    match options {
        SessionConfigSelectOptions::Ungrouped(values) => values
            .iter()
            .map(|value| ConfigOptionValueView {
                value: value.value.to_string(),
                name: value.name.clone(),
            })
            .collect(),
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| {
                group.options.iter().map(|value| ConfigOptionValueView {
                    value: value.value.to_string(),
                    name: value.name.clone(),
                })
            })
            .collect(),
        _ => Vec::new(),
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
    fn stale_role_skips_when_option_absent() {
        let options = vec![view("llm", Some("model"))];
        assert_eq!(
            option_id_for_role(&options, crate::repo_state::ConfigRole::Model),
            Some("llm".to_string())
        );
        assert_eq!(
            option_id_for_role(&options, crate::repo_state::ConfigRole::Effort),
            None
        );
    }
}
