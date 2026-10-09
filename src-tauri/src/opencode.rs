// OpenCode-specific edge driver: agent definitions injected at spawn plus
// first-message role templates. `acp.rs` stays protocol-only.
use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::plans::{Phase, PlanRef};

pub const SCOPING_AGENT: &str = "samokod-scoping";
pub const EXECUTING_AGENT: &str = "samokod-executing";
pub const EVERGREENING_AGENT: &str = "samokod-evergreening";
pub const LANDING_AGENT: &str = "samokod-landing";

/// Matches every plan's state file. Each agent's edit rules deny it last.
const STATE_DENY_GLOB: &str = ".samokod/plans/**/state.json";

/// Agent id for a phase. Pure.
pub fn agent_for(phase: Phase) -> &'static str {
    match phase {
        Phase::Scoping => SCOPING_AGENT,
        Phase::Executing => EXECUTING_AGENT,
        Phase::Evergreening => EVERGREENING_AGENT,
        Phase::Landing => LANDING_AGENT,
        Phase::Completed | Phase::Cancelled => EXECUTING_AGENT,
    }
}

/// Env key carrying inline JSON config. Merges over user and project config.
pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";

const SCOPING_PROMPT: &str = include_str!("prompts/scoping.md");
const EXECUTING_PROMPT: &str = include_str!("prompts/executing.md");
const EVERGREENING_PROMPT: &str = include_str!("prompts/evergreening.md");
const LANDING_PROMPT: &str = include_str!("prompts/landing.md");

/// Extra spawn env pinning all agents. Pure: JSON only, no process access.
pub fn agent_env(plan: &PlanRef) -> HashMap<String, String> {
    HashMap::from([(CONFIG_CONTENT_ENV.to_string(), agent_config(plan))])
}

/// Full agent config as inline JSON. Permissions only, no `prompt` field:
/// role guidance travels as first-message text so OpenCode's per-model base
/// prompt stays intact. Pure.
pub fn agent_config(plan: &PlanRef) -> String {
    serde_json::json!({
        "agent": {
            SCOPING_AGENT: {
                "mode": "primary",
                "description": "Plans one task. Writes only inside its plan directory.",
                "permission": scoping_permissions(plan),
            },
            EXECUTING_AGENT: {
                "mode": "primary",
                "description": "Executes one approved plan.",
                "permission": executing_permissions(),
            },
            EVERGREENING_AGENT: {
                "mode": "primary",
                "description": "Cleans up one executed plan.",
                "permission": evergreening_permissions(),
            },
            LANDING_AGENT: {
                "mode": "primary",
                "description": "Lands one plan branch onto its target branch.",
                "permission": landing_permissions(),
            },
        },
    })
    .to_string()
}

/// Scoping edits stay inside its plan scope, except the state file; the
/// shell stays fully allowed for investigation.
fn scoping_permissions(plan: &PlanRef) -> Value {
    let scope = plan.scope_glob();
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": rules_object(&[
            ("*", "deny"),
            (scope.as_str(), "allow"),
            (STATE_DENY_GLOB, "deny"),
        ]),
        "bash": "allow",
        "question": "deny",
        "task": rules_object(&SCOPING_TASK_RULES),
    })
}

/// Scoping may only spawn read-only researchers. Denied types vanish from
/// the Task tool description, so the model will not attempt them.
const SCOPING_TASK_RULES: [(&str, &str); 3] =
    [("*", "deny"), ("explore", "allow"), ("scout", "allow")];

/// Executing runs an approved plan, so the shell is fully allowed. Edits
/// ask inside `.samokod` to stop casual rewrites of app state through the
/// obvious path; `bash` bypass is accepted. The bare `.samokod` key covers
/// the directory entry itself, since `.samokod/**` only matches paths
/// starting with `.samokod/`, and state files deny.
fn executing_permissions() -> Value {
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": rules_object(&EXECUTING_EDIT_RULES),
        "bash": "allow",
        "question": "deny",
    })
}

const EXECUTING_EDIT_RULES: [(&str, &str); 4] = [
    ("*", "allow"),
    (".samokod", "ask"),
    (".samokod/**", "ask"),
    (STATE_DENY_GLOB, "deny"),
];

/// Evergreening runs in the same worktree as executing, so it shares the
/// executing edit shape: everything editable except the state file, with
/// the `.samokod` ask guard and no permission questions.
fn evergreening_permissions() -> Value {
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": rules_object(&EVERGREENING_EDIT_RULES),
        "bash": "allow",
        "question": "deny",
    })
}

const EVERGREENING_EDIT_RULES: [(&str, &str); 4] = [
    ("*", "allow"),
    (".samokod", "ask"),
    (".samokod/**", "ask"),
    (STATE_DENY_GLOB, "deny"),
];

/// Landing resolves one plan branch onto its target, so every worktree
/// file is editable except the state file: a landed commit must cover
/// files the target branch added on top. No permission questions and no
/// subagent research: landing is a focused conflict-resolution task.
fn landing_permissions() -> Value {
    serde_json::json!({
        "read": "allow",
        "external_directory": "allow",
        "edit": rules_object(&LANDING_EDIT_RULES),
        "bash": "allow",
        "question": "deny",
        "task": "deny",
    })
}

const LANDING_EDIT_RULES: [(&str, &str); 2] = [("*", "allow"), (STATE_DENY_GLOB, "deny")];

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

/// Scoping template with its plan dir filled in. Rendered as its own
/// bubble; the first scoping prompt carries it on the wire. Pure.
pub fn scoping_template(plan_dir: &str) -> String {
    SCOPING_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

/// Executing role and instruction. Sent once per executing conversation:
/// as the full first turn on approval, prefixed to the first prompt
/// otherwise. Pure.
pub fn executing_first_message(plan_dir: &str) -> String {
    EXECUTING_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

/// Evergreening role and instruction. Sent as the full first turn when
/// the cleanup path starts the evergreening agent in the same worktree.
/// Pure.
pub fn evergreening_first_message(
    worktree_path: &str,
    worktree_branch: &str,
    plan_md_abs_path: &str,
    target_branch: &str,
    commits: &str,
) -> String {
    EVERGREENING_PROMPT
        .replace("{{WORKTREE_PATH}}", worktree_path)
        .replace("{{WORKTREE_BRANCH}}", worktree_branch)
        .replace("{{PLAN_MD_ABS_PATH}}", plan_md_abs_path)
        .replace("{{TARGET_BRANCH}}", target_branch)
        .replace("{{COMMITS}}", commits)
}

/// Landing role and instruction. Sent as the full first turn when the
/// conflict path starts the landing agent in the worktree. Pure.
pub fn landing_first_message(
    worktree_branch: &str,
    target_branch: &str,
    worktree_path: &str,
    plan_md_abs_path: &str,
) -> String {
    LANDING_PROMPT
        .replace("{{WORKTREE_BRANCH}}", worktree_branch)
        .replace("{{TARGET_BRANCH}}", target_branch)
        .replace("{{WORKTREE_PATH}}", worktree_path)
        .replace("{{PLAN_MD_ABS_PATH}}", plan_md_abs_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::STATE_FILE;

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
        for agent in [
            SCOPING_AGENT,
            EXECUTING_AGENT,
            EVERGREENING_AGENT,
            LANDING_AGENT,
        ] {
            let entry = config
                .pointer(&format!("/agent/{agent}"))
                .expect("agent present");
            assert_eq!(entry["mode"], "primary");
            assert!(entry.get("prompt").is_none(), "{agent} must not set prompt");
        }
    }

    #[test]
    fn scoping_edit_allows_only_plan_dir() {
        let plan = test_plan();
        let config = config(&plan);
        let inside = format!(".samokod/plans/scoping/{}/plan.md", plan.name);
        assert_eq!(effect_of(&config, SCOPING_AGENT, "edit", &inside), "allow");
        assert_eq!(
            effect_of(&config, SCOPING_AGENT, "edit", "src/App.tsx"),
            "deny"
        );
    }

    #[test]
    fn scoping_allows_bash() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, SCOPING_AGENT, "bash", "cargo test"),
            "allow"
        );
    }

    #[test]
    fn scoping_task_allows_explore_and_scout() {
        let config = config(&test_plan());
        for agent_type in ["explore", "scout"] {
            assert_eq!(
                effect_of(&config, SCOPING_AGENT, "task", agent_type),
                "allow",
                "{agent_type} should be allowed"
            );
        }
        for agent_type in ["general", "unknown-type"] {
            assert_eq!(
                effect_of(&config, SCOPING_AGENT, "task", agent_type),
                "deny",
                "{agent_type} should be denied"
            );
        }
    }

    #[test]
    fn executing_allows_edits_and_bash() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, EXECUTING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, EXECUTING_AGENT, "bash", "cargo test"),
            "allow"
        );
    }

    #[test]
    fn executing_asks_inside_samokod() {
        let config = config(&test_plan());
        for resource in [
            ".samokod",
            ".samokod/state.json",
            ".samokod/plans/scoping/x/plan.md",
        ] {
            assert_eq!(
                effect_of(&config, EXECUTING_AGENT, "edit", resource),
                "ask",
                "{resource} should ask"
            );
        }
        assert_eq!(
            effect_of(&config, EXECUTING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
    }

    #[test]
    fn granular_rules_serialize_in_order() {
        let config = config(&test_plan());
        let plan = test_plan();
        assert_eq!(
            keys_of(&config, SCOPING_AGENT, "edit"),
            vec![
                "*".to_string(),
                plan.scope_glob(),
                STATE_DENY_GLOB.to_string()
            ]
        );
        assert_eq!(
            keys_of(&config, SCOPING_AGENT, "task"),
            vec!["*".to_string(), "explore".to_string(), "scout".to_string()]
        );
        assert_eq!(
            keys_of(&config, EXECUTING_AGENT, "edit"),
            vec![
                "*".to_string(),
                ".samokod".to_string(),
                ".samokod/**".to_string(),
                STATE_DENY_GLOB.to_string()
            ]
        );
        assert_eq!(
            keys_of(&config, EVERGREENING_AGENT, "edit"),
            vec![
                "*".to_string(),
                ".samokod".to_string(),
                ".samokod/**".to_string(),
                STATE_DENY_GLOB.to_string()
            ]
        );
        assert_eq!(
            keys_of(&config, LANDING_AGENT, "edit"),
            vec!["*".to_string(), STATE_DENY_GLOB.to_string()]
        );
    }

    #[test]
    fn scoping_and_executing_deny_question() {
        let config = config(&test_plan());
        assert_eq!(
            config["agent"][SCOPING_AGENT]["permission"]["question"],
            "deny"
        );
        assert_eq!(
            config["agent"][EXECUTING_AGENT]["permission"]["question"],
            "deny"
        );
        assert_eq!(
            config["agent"][EVERGREENING_AGENT]["permission"]["question"],
            "deny"
        );
        assert_eq!(
            config["agent"][LANDING_AGENT]["permission"]["question"],
            "deny"
        );
    }

    #[test]
    fn landing_allows_worktree_edits_without_subagents() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, LANDING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, LANDING_AGENT, "edit", ".samokod/state.json"),
            "allow"
        );
        assert_eq!(
            effect_of(
                &config,
                LANDING_AGENT,
                "edit",
                &format!(".samokod/plans/landing/x/{STATE_FILE}")
            ),
            "deny"
        );
        assert_eq!(
            effect_of(&config, LANDING_AGENT, "bash", "git rebase -i feature"),
            "allow"
        );
        assert_eq!(config["agent"][LANDING_AGENT]["permission"]["task"], "deny");
    }

    #[test]
    fn landing_plans_run_the_landing_agent() {
        assert_eq!(agent_for(Phase::Landing), LANDING_AGENT);
        assert_eq!(agent_for(Phase::Evergreening), EVERGREENING_AGENT);
        assert_eq!(agent_for(Phase::Executing), EXECUTING_AGENT);
        assert_eq!(agent_for(Phase::Scoping), SCOPING_AGENT);
    }

    #[test]
    fn evergreening_allows_edits_and_bash() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, EVERGREENING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, EVERGREENING_AGENT, "bash", "cargo test"),
            "allow"
        );
    }

    #[test]
    fn state_file_denies_last_for_every_agent() {
        let plan = test_plan();
        let config = config(&plan);
        let own = format!(".samokod/plans/scoping/{}/{STATE_FILE}", plan.name);
        assert_eq!(effect_of(&config, SCOPING_AGENT, "edit", &own), "deny");
        for resource in [
            format!(".samokod/plans/scoping/other/{STATE_FILE}"),
            format!(".samokod/plans/executing/x/{STATE_FILE}"),
            format!(".samokod/plans/evergreening/x/{STATE_FILE}"),
        ] {
            assert_eq!(
                effect_of(&config, SCOPING_AGENT, "edit", &resource),
                "deny",
                "{resource} should deny for scoping"
            );
            assert_eq!(
                effect_of(&config, EXECUTING_AGENT, "edit", &resource),
                "deny",
                "{resource} should deny for executing"
            );
            assert_eq!(
                effect_of(&config, EVERGREENING_AGENT, "edit", &resource),
                "deny",
                "{resource} should deny for evergreening"
            );
            assert_eq!(
                effect_of(&config, LANDING_AGENT, "edit", &resource),
                "deny",
                "{resource} should deny for landing"
            );
        }
    }

    #[test]
    fn evergreening_message_fills_every_placeholder() {
        let message = evergreening_first_message(
            "/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature",
            "samokod/shiny-feature",
            "/repo/.samokod/plans/evergreening/2026-09-26.14-53-26.shiny-feature/plan.md",
            "feature",
            "0923748 feat: blabla\n009fb12 fix: blabla",
        );
        assert!(message.contains("/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature"));
        assert!(message.contains("samokod/shiny-feature"));
        assert!(message.contains(
            "/repo/.samokod/plans/evergreening/2026-09-26.14-53-26.shiny-feature/plan.md"
        ));
        assert!(message.contains("feature"));
        assert!(message.contains("0923748 feat: blabla"));
        assert!(message.contains("009fb12 fix: blabla"));
        assert!(message.contains("Use TODOs to track progress"));
        assert!(!message.contains("{{WORKTREE_PATH}}"));
        assert!(!message.contains("{{WORKTREE_BRANCH}}"));
        assert!(!message.contains("{{PLAN_MD_ABS_PATH}}"));
        assert!(!message.contains("{{TARGET_BRANCH}}"));
        assert!(!message.contains("{{COMMITS}}"));
    }

    #[test]
    fn landing_message_fills_every_placeholder() {
        let message = landing_first_message(
            "samokod/shiny-feature",
            "feature",
            "/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature",
            "/repo/.samokod/plans/landing/2026-09-26.14-53-26.shiny-feature/plan.md",
        );
        assert!(message.contains("samokod/shiny-feature"));
        assert!(message.contains("`feature`"));
        assert!(message.contains("/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature"));
        assert!(!message.contains("{{WORKTREE_BRANCH}}"));
        assert!(!message.contains("{{TARGET_BRANCH}}"));
        assert!(!message.contains("{{WORKTREE_PATH}}"));
        assert!(!message.contains("{{PLAN_MD_ABS_PATH}}"));
    }

    #[test]
    fn scoping_template_names_plan_path() {
        let draft = scoping_template(".samokod/plans/scoping/ts");
        assert!(draft.contains(".samokod/plans/scoping/ts/plan.md"));
        assert!(!draft.contains("{{PLAN_DIR}}"));
    }

    #[test]
    fn executing_message_names_plan_path() {
        let message = executing_first_message(".samokod/plans/executing/ts.slug");
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
