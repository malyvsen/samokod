// OpenCode-specific edge driver: agent definitions injected at spawn plus
// first-message role templates. `acp.rs` stays protocol-only.
use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::plans::{Phase, PlanRef};

pub const PLANNER_AGENT: &str = "samokod-planner";
pub const EXECUTOR_AGENT: &str = "samokod-executor";
pub const MERGER_AGENT: &str = "samokod-merger";

/// Agent id for a phase. Pure.
pub fn agent_for(phase: Phase) -> &'static str {
    match phase {
        Phase::Scoping => PLANNER_AGENT,
        Phase::Executing => EXECUTOR_AGENT,
        Phase::Merging => MERGER_AGENT,
        Phase::Completed | Phase::Cancelled => EXECUTOR_AGENT,
    }
}

/// Env key carrying inline JSON config. Merges over user and project config.
pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";

const PLANNER_PROMPT: &str = include_str!("prompts/planner.md");
const EXECUTOR_PROMPT: &str = include_str!("prompts/executor.md");
const MERGER_PROMPT: &str = include_str!("prompts/merger.md");

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
            MERGER_AGENT: {
                "mode": "primary",
                "description": "Rebases one plan branch onto the latest main.",
                "permission": merger_permissions(),
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

/// Merger rebases one plan branch, so every worktree file is editable:
/// a rebased commit must cover files the main branch added on top. No
/// permission questions and no subagent research: merging is a focused
/// rebasing task.
fn merger_permissions() -> Value {
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": "allow",
        "bash": "allow",
        "question": "deny",
        "task": "deny",
    })
}

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

/// Scoping draft: the planner template with its plan dir filled in.
/// Prefilled into the first message box as ordinary editable user content.
/// Pure.
pub fn scoping_draft(plan_dir: &str) -> String {
    PLANNER_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

/// Executor role and instruction. Sent once per executing conversation:
/// hidden on approval, prefixed to the first prompt otherwise. Pure.
pub fn executor_first_message(plan_dir: &str) -> String {
    EXECUTOR_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

/// Merger role and instruction. Sent hidden when the conflict path starts
/// the merge agent in the worktree. Pure.
pub fn merger_first_message(
    worktree_branch: &str,
    main_branch: &str,
    worktree_path: &str,
    plan_md_abs_path: &str,
) -> String {
    MERGER_PROMPT
        .replace("{{WORKTREE_BRANCH}}", worktree_branch)
        .replace("{{MAIN_BRANCH}}", main_branch)
        .replace("{{WORKTREE_PATH}}", worktree_path)
        .replace("{{PLAN_MD_ABS_PATH}}", plan_md_abs_path)
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
    fn all_agents_are_primary_without_prompt_field() {
        let config = config(&test_plan());
        for agent in [PLANNER_AGENT, EXECUTOR_AGENT, MERGER_AGENT] {
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
        assert_eq!(
            config["agent"][MERGER_AGENT]["permission"]["question"],
            "deny"
        );
    }

    #[test]
    fn merger_allows_every_worktree_edit_without_subagents() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, MERGER_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, MERGER_AGENT, "edit", ".samokod/state.json"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, MERGER_AGENT, "bash", "git rebase -i main"),
            "allow"
        );
        assert_eq!(config["agent"][MERGER_AGENT]["permission"]["task"], "deny");
    }

    #[test]
    fn merging_plans_run_the_merger() {
        assert_eq!(agent_for(Phase::Merging), MERGER_AGENT);
        assert_eq!(agent_for(Phase::Executing), EXECUTOR_AGENT);
        assert_eq!(agent_for(Phase::Scoping), PLANNER_AGENT);
    }

    #[test]
    fn merger_message_fills_every_placeholder() {
        let message = merger_first_message(
            "samokod/shiny-feature",
            "main",
            "/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature",
            "/repo/.samokod/plans/merging/2026-09-26.14-53-26.shiny-feature/plan.md",
        );
        assert!(message.contains("samokod/shiny-feature"));
        assert!(message.contains("`main`"));
        assert!(message.contains("/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature"));
        assert!(!message.contains("{{WORKTREE_BRANCH}}"));
        assert!(!message.contains("{{MAIN_BRANCH}}"));
        assert!(!message.contains("{{WORKTREE_PATH}}"));
        assert!(!message.contains("{{PLAN_MD_ABS_PATH}}"));
    }

    #[test]
    fn scoping_draft_names_plan_path() {
        let draft = scoping_draft(".samokod/plans/scoping/ts");
        assert!(draft.contains(".samokod/plans/scoping/ts/plan.md"));
        assert!(!draft.contains("{{PLAN_DIR}}"));
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
