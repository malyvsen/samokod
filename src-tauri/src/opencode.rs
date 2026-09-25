// OpenCode-specific edge driver: agent definitions injected at spawn plus
// first-message role templates. `acp.rs` stays protocol-only.
use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::plans::{Phase, PlanRef};

pub const PLANNER_AGENT: &str = "samokod-planner";
pub const EXECUTOR_AGENT: &str = "samokod-executor";

/// Agent id for a phase. Pure.
pub fn agent_for(phase: Phase) -> &'static str {
    match phase {
        Phase::Scoping => PLANNER_AGENT,
        Phase::Executing | Phase::Completed | Phase::Cancelled => EXECUTOR_AGENT,
    }
}

/// Env key carrying inline JSON config. Merges over user and project config.
pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";

const PLANNER_PROMPT: &str = include_str!("prompts/planner.md");
const EXECUTOR_PROMPT: &str = include_str!("prompts/executor.md");

/// Extra spawn env pinning both agents. Pure: JSON only, no process access.
pub fn agent_env(plan: &PlanRef) -> HashMap<String, String> {
    HashMap::from([(CONFIG_CONTENT_ENV.to_string(), agent_config(plan))])
}

/// Full agent config as inline JSON. Permissions only, no `prompt` field:
/// role guidance travels as first-message text so OpenCode's per-model base
/// prompt stays intact. Pure.
pub fn agent_config(plan: &PlanRef) -> String {
    serde_json::json!({
        "agent": {
            PLANNER_AGENT: {
                "mode": "primary",
                "description": "Plans one task. Writes only inside its plan directory.",
                "permission": planner_permissions(plan),
            },
            EXECUTOR_AGENT: {
                "mode": "primary",
                "description": "Executes one approved plan.",
                "permission": executor_permissions(),
            },
        },
    })
    .to_string()
}

/// Planner edits stay inside its relative plan scope; the shell stays fully
/// allowed for investigation. Rule order is load-bearing: OpenCode grants
/// the last matching rule, so `*` comes first and the scope after.
fn planner_permissions(plan: &PlanRef) -> Value {
    let mut edit = Map::with_capacity(2);
    edit.insert("*".to_string(), Value::String("deny".to_string()));
    edit.insert(plan.scope_glob(), Value::String("allow".to_string()));
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": Value::Object(edit),
        "bash": "allow",
        "question": "deny",
        "task": rules_object(&PLANNER_TASK_RULES),
    })
}

/// Planner may only spawn read-only researchers. Denied types vanish from
/// the Task tool description, so the model will not attempt them.
const PLANNER_TASK_RULES: [(&str, &str); 3] =
    [("*", "deny"), ("explore", "allow"), ("scout", "allow")];

/// Executor runs an approved plan, so the shell is fully allowed. Edits
/// ask inside `.samokod` to stop casual rewrites of app state through the
/// obvious path; `bash` bypass is accepted. The bare `.samokod` key covers
/// the directory entry itself, since `.samokod/**` only matches paths
/// starting with `.samokod/`. Order is load-bearing, `*` first.
fn executor_permissions() -> Value {
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": rules_object(&EXECUTOR_EDIT_RULES),
        "bash": "allow",
        "question": "deny",
    })
}

const EXECUTOR_EDIT_RULES: [(&str, &str); 3] =
    [("*", "allow"), (".samokod", "ask"), (".samokod/**", "ask")];

/// Ordered permission object from `(pattern, effect)` pairs. Insertion
/// order is the contract: OpenCode grants the last matching rule.
fn rules_object(rules: &[(&str, &str)]) -> Value {
    let mut map = Map::with_capacity(rules.len());
    for (pattern, effect) in rules {
        map.insert((*pattern).to_string(), Value::String((*effect).to_string()));
    }
    Value::Object(map)
}

/// Plan dir as the agent sees it: repo-relative, forward slashes. Pure.
pub fn plan_display(plan: &PlanRef) -> String {
    format!(".samokod/plans/{}/{}", plan.phase.dir_name(), plan.name)
}

/// Planner role plus the user's own first message. Sent once per scoping
/// chat; later messages go through untouched. Pure.
pub fn planner_first_message(plan_dir: &str, user_text: &str) -> String {
    format!(
        "{}\n\n{}",
        PLANNER_PROMPT.replace("{{PLAN_DIR}}", plan_dir),
        user_text.trim()
    )
}

/// Unified executor role and instruction. Sent as the hidden first prompt of
/// every executing chat. Pure.
pub fn executor_first_message(plan_dir: &str) -> String {
    EXECUTOR_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_plan() -> PlanRef {
        PlanRef {
            name: "ts".to_string(),
            phase: Phase::Scoping,
        }
    }

    fn config(plan: &PlanRef) -> Value {
        serde_json::from_str(&agent_config(plan)).expect("valid JSON")
    }

    /// Mirror of OpenCode's find-last wildcard match: the last rule whose
    /// pattern matches the resource wins. String permissions apply directly.
    fn effect_of(config: &Value, agent: &str, permission: &str, resource: &str) -> String {
        let entry = &config["agent"][agent]["permission"][permission];
        if let Some(effect) = entry.as_str() {
            return effect.to_string();
        }
        let rules = entry.as_object().expect("permission is string or object");
        let mut effect = "none";
        for (pattern, value) in rules {
            if wildcard_match(pattern, resource) {
                effect = value.as_str().expect("rule effect is a string");
            }
        }
        effect.to_string()
    }

    /// `*` crosses `/`, like OpenCode's wildcard. Pure.
    fn wildcard_match(pattern: &str, text: &str) -> bool {
        let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
        let (mut pi, mut ti) = (0, 0);
        let (mut star, mut restart) = (None, 0);
        while ti < text.len() {
            if pi < pattern.len() && pattern[pi] == b'*' {
                star = Some(pi);
                restart = ti;
                pi += 1;
            } else if pi < pattern.len() && pattern[pi] == text[ti] {
                pi += 1;
                ti += 1;
            } else if let Some(star_at) = star {
                restart += 1;
                ti = restart;
                pi = star_at + 1;
            } else {
                return false;
            }
        }
        while pi < pattern.len() && pattern[pi] == b'*' {
            pi += 1;
        }
        pi == pattern.len()
    }

    fn keys_of(config: &Value, agent: &str, permission: &str) -> Vec<String> {
        config["agent"][agent]["permission"][permission]
            .as_object()
            .expect("granular permission is an object")
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn both_agents_are_primary_without_prompt_field() {
        let config = config(&test_plan());
        for agent in [PLANNER_AGENT, EXECUTOR_AGENT] {
            let entry = config
                .pointer(&format!("/agent/{agent}"))
                .expect("agent present");
            assert_eq!(entry["mode"], "primary");
            assert!(entry.get("prompt").is_none(), "{agent} must not set prompt");
        }
    }

    #[test]
    fn planner_edit_allows_only_plan_dir() {
        let plan = test_plan();
        let config = config(&plan);
        let inside = format!(".samokod/plans/scoping/{}/plan.md", plan.name);
        assert_eq!(effect_of(&config, PLANNER_AGENT, "edit", &inside), "allow");
        assert_eq!(
            effect_of(&config, PLANNER_AGENT, "edit", "src/App.tsx"),
            "deny"
        );
    }

    #[test]
    fn planner_allows_bash() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, PLANNER_AGENT, "bash", "cargo test"),
            "allow"
        );
    }

    #[test]
    fn planner_task_allows_explore_and_scout() {
        let config = config(&test_plan());
        for agent_type in ["explore", "scout"] {
            assert_eq!(
                effect_of(&config, PLANNER_AGENT, "task", agent_type),
                "allow",
                "{agent_type} should be allowed"
            );
        }
        for agent_type in ["general", "unknown-type"] {
            assert_eq!(
                effect_of(&config, PLANNER_AGENT, "task", agent_type),
                "deny",
                "{agent_type} should be denied"
            );
        }
    }

    #[test]
    fn executor_allows_edits_and_bash() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, EXECUTOR_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, EXECUTOR_AGENT, "bash", "cargo test"),
            "allow"
        );
    }

    #[test]
    fn executor_asks_inside_samokod() {
        let config = config(&test_plan());
        for resource in [
            ".samokod",
            ".samokod/state.json",
            ".samokod/plans/scoping/x/plan.md",
        ] {
            assert_eq!(
                effect_of(&config, EXECUTOR_AGENT, "edit", resource),
                "ask",
                "{resource} should ask"
            );
        }
        assert_eq!(
            effect_of(&config, EXECUTOR_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
    }

    #[test]
    fn granular_rules_serialize_deny_first() {
        let config = config(&test_plan());
        let plan = test_plan();
        assert_eq!(
            keys_of(&config, PLANNER_AGENT, "edit"),
            vec!["*".to_string(), plan.scope_glob()]
        );
        assert_eq!(
            keys_of(&config, PLANNER_AGENT, "task"),
            vec!["*".to_string(), "explore".to_string(), "scout".to_string()]
        );
        assert_eq!(
            keys_of(&config, EXECUTOR_AGENT, "edit"),
            vec![
                "*".to_string(),
                ".samokod".to_string(),
                ".samokod/**".to_string()
            ]
        );
    }

    #[test]
    fn planner_and_executor_deny_question() {
        let config = config(&test_plan());
        assert_eq!(
            config["agent"][PLANNER_AGENT]["permission"]["question"],
            "deny"
        );
        assert_eq!(
            config["agent"][EXECUTOR_AGENT]["permission"]["question"],
            "deny"
        );
    }

    #[test]
    fn planner_message_combines_role_and_user_text() {
        let message = planner_first_message(".samokod/plans/scoping/ts", "  do things  ");
        assert!(message.contains(".samokod/plans/scoping/ts/plan.md"));
        assert!(!message.contains("{{PLAN_DIR}}"));
        assert!(message.ends_with("do things"));
    }

    #[test]
    fn executor_message_names_plan_path() {
        let message = executor_first_message(".samokod/plans/executing/ts.slug");
        assert!(message.contains(".samokod/plans/executing/ts.slug/plan.md"));
        assert!(!message.contains("{{PLAN_DIR}}"));
    }

    #[test]
    fn plan_display_names_phase_dir() {
        let scoping = PlanRef {
            name: "ts".to_string(),
            phase: Phase::Scoping,
        };
        assert_eq!(plan_display(&scoping), ".samokod/plans/scoping/ts");
        let executing = PlanRef {
            name: "ts.slug".to_string(),
            phase: Phase::Executing,
        };
        assert_eq!(plan_display(&executing), ".samokod/plans/executing/ts.slug");
    }
}
