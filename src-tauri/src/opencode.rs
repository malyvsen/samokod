// OpenCode-specific edge driver: agent definitions injected at spawn plus
// first-message role templates. `acp.rs` stays protocol-only.
use std::collections::HashMap;
use std::path::Path;

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

/// Extra spawn env pinning both agents. Planner edits stay inside
/// `scope_glob` (absolute plan dir plus `/**`); everything else is static.
/// Pure: JSON only, no process access.
pub fn agent_env(scope_glob: &str) -> HashMap<String, String> {
    HashMap::from([(CONFIG_CONTENT_ENV.to_string(), agent_config(scope_glob))])
}

/// Full agent config as inline JSON. Permissions only, no `prompt` field:
/// role guidance travels as first-message text so OpenCode's per-model base
/// prompt stays intact. Pure.
pub fn agent_config(scope_glob: &str) -> String {
    serde_json::json!({
        "agent": {
            PLANNER_AGENT: {
                "mode": "primary",
                "description": "Plans one task. Writes only inside its plan directory.",
                "permission": {
                    "read": "allow",
                    "external_directory": "allow",
                    "edit": { "*": "deny", scope_glob: "allow" },
                    "bash": "allow",
                    "question": "deny",
                    "task": { "general": "deny" },
                },
            },
            EXECUTOR_AGENT: {
                "mode": "primary",
                "description": "Executes one approved plan.",
                "permission": {
                    "read": "allow",
                    "external_directory": "allow",
                    "edit": "allow",
                    "bash": "ask",
                    "question": "deny",
                },
            },
        },
    })
    .to_string()
}

/// Absolute edit scope for one plan dir. Pure.
pub fn scope_glob(repo_root: &Path, phase: Phase, plan_name: &str) -> String {
    format!(
        "{}/.samokod/plans/{}/{}/**",
        repo_root.display(),
        phase.dir_name(),
        plan_name
    )
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
    use serde_json::Value;

    fn config(scope: &str) -> Value {
        serde_json::from_str(&agent_config(scope)).expect("valid JSON")
    }

    #[test]
    fn both_agents_are_primary_without_prompt_field() {
        let config = config("/repo/.samokod/plans/scoping/ts/**");
        for agent in [PLANNER_AGENT, EXECUTOR_AGENT] {
            let entry = config
                .pointer(&format!("/agent/{agent}"))
                .expect("agent present");
            assert_eq!(entry["mode"], "primary");
            assert!(entry.get("prompt").is_none(), "{agent} must not set prompt");
        }
    }

    #[test]
    fn planner_confines_edits_to_scope() {
        let scope = "/repo/.samokod/plans/scoping/ts/**";
        let edit = &config(scope)["agent"][PLANNER_AGENT]["permission"]["edit"];
        assert_eq!(edit["*"], "deny");
        assert_eq!(edit[scope], "allow");
    }

    #[test]
    fn planner_and_executor_deny_question() {
        let config = config("/scope/**");
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
    fn executor_allows_edits_and_asks_bash() {
        let config = config("/scope/**");
        let permission = &config["agent"][EXECUTOR_AGENT]["permission"];
        assert_eq!(permission["edit"], "allow");
        assert_eq!(permission["bash"], "ask");
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
