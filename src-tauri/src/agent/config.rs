// Session config: reading and reapplying model/effort options per
// session, with the repo-wide default in `.samokod/state.json`.
use crate::acp::{
    self, Agent, ConnectionTo, SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions,
};
use crate::types::{AgentError, ConfigOptionValueView, ConfigOptionView, SessionKey};

use super::AgentManager;

impl AgentManager {
    /// Set one session config option without restarting the session.
    /// Returns the agent's complete option list, including dependent updates.
    /// Also stores the choice as the repo-wide default new sessions start
    /// from.
    pub async fn set_config_option(
        &self,
        session: SessionKey,
        config_id: String,
        value: String,
    ) -> Result<Vec<ConfigOptionView>, AgentError> {
        let (connection, session_id, _, _) =
            self.session_snapshot_for(&session)
                .ok_or_else(|| AgentError::NoSession {
                    raw: "open a repository first".to_string(),
                })?;
        let response = send_config_option(
            &connection,
            &acp::SessionId::new(session_id),
            &config_id,
            &value,
        )
        .await
        .map_err(|error| AgentError::RequestFailed { raw: error })?;
        // Re-sync both roles from the full response list: dependent options
        // can vanish in the same response, clearing the stored role.
        let roles = crate::repo_state::roles_from_options(&response);
        let moved = match self.state.lock() {
            Ok(mut state) => {
                let current_roles = state
                    .sessions
                    .get(&session)
                    .map(|live| live.last_roles.clone())
                    .unwrap_or_default();
                let moved: Vec<crate::repo_state::ConfigRole> = crate::repo_state::ConfigRole::ALL
                    .into_iter()
                    .filter(|role| role.get(&current_roles) != role.get(&roles))
                    .collect();
                if let Some(live) = state.sessions.get_mut(&session) {
                    live.last_roles = roles.clone();
                }
                Some(moved)
            }
            Err(error) => {
                log::warn!("failed to remember config choice: {error}");
                None
            }
        };
        if let (Some(moved), Some(repo)) = (moved, self.current_repo()) {
            for role in moved {
                crate::repo_state::set_role(&repo, role, role.get(&roles).map(String::as_str));
            }
        }
        log_roles("selected", &roles);
        Ok(response)
    }
}

/// Select one agent by mode id. Fails loud: a planner session running as
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
pub(crate) fn option_id_for_role(
    options: &[ConfigOptionView],
    role: crate::repo_state::ConfigRole,
) -> Option<String> {
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
pub(crate) fn log_roles(action: &str, roles: &crate::repo_state::RepoState) {
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
