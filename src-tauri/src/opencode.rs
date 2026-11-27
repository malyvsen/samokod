// OpenCode-specific edge driver: agent definitions injected at spawn plus
// role instructions via an `instructions` file. `acp.rs` stays protocol-only.
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::plans::PlanRef;
use crate::types::{SessionKey, SessionRole};

pub const SCOPING_AGENT: &str = "samokod-scoping";
pub const EXECUTING_AGENT: &str = "samokod-executing";
pub const EVERGREENING_AGENT: &str = "samokod-evergreening";
pub const REBASING_AGENT: &str = "samokod-rebasing";

/// Matches every plan's state file. Each agent's edit rules deny it last.
const STATE_DENY_GLOB: &str = ".samokod/plans/**/state.json";

/// Matches every plan's image attachments. Agents can read them but never
/// edit them, denied last so deny wins.
const ATTACHMENTS_DENY_GLOB: &str = ".samokod/plans/**/attachments/**";

/// Agent id for a session role. Pure.
pub fn agent_for(role: SessionRole) -> &'static str {
    match role {
        SessionRole::Scoping => SCOPING_AGENT,
        SessionRole::Executing => EXECUTING_AGENT,
        SessionRole::Evergreening => EVERGREENING_AGENT,
        SessionRole::Rebasing => REBASING_AGENT,
    }
}

/// Env key carrying inline JSON config. Merges over user and project config.
pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";

const SCOPING_PROMPT: &str = include_str!("prompts/scoping.md");
const EXECUTING_PROMPT: &str = include_str!("prompts/executing.md");
const EVERGREENING_PROMPT: &str = include_str!("prompts/evergreening.md");
const REBASING_PROMPT: &str = include_str!("prompts/rebasing.md");

/// Extra spawn env pinning all agents plus a per-session instructions
/// file carrying the role instructions. Writes the file so it exists before
/// the process spawns. Pure apart from the file write at the spawn edge.
pub fn session_env(
    window: &str,
    key: &SessionKey,
    plan: &PlanRef,
    system: &str,
) -> HashMap<String, String> {
    let instructions = instructions_path(window, key);
    write_instructions_file(&instructions, system);
    agent_env(plan, &instructions)
}

/// Extra spawn env pinning all agents. Pure: JSON only, no process access.
pub fn agent_env(plan: &PlanRef, instructions: &Path) -> HashMap<String, String> {
    HashMap::from([(
        CONFIG_CONTENT_ENV.to_string(),
        agent_config(plan, instructions),
    )])
}

/// Full agent config as inline JSON. Permissions only, no `system` field:
/// role guidance travels via a per-session `instructions` file so OpenCode's
/// per-model base prompt stays intact. The path must be absolute;
/// OpenCode resolves relative `instructions` against the workspace.
/// Native V2 shape: `agents` plus an ordered
/// `permissions` array per agent (last match wins), `shell` for shell
/// commands, `subagent` for child agents, and `experimental.todo` so the
/// `todowrite` tool exists (off by default in V2). Pure.
pub fn agent_config(plan: &PlanRef, instructions: &Path) -> String {
    let mut root = serde_json::json!({
        "experimental": { "todo": true },
        "agents": {
            SCOPING_AGENT: {
                "mode": "primary",
                "description": "Plans one task. Writes only inside its plan directory.",
                "permissions": scoping_permissions(plan),
            },
            EXECUTING_AGENT: {
                "mode": "primary",
                "description": "Executes one approved plan.",
                "permissions": executing_permissions(),
            },
            EVERGREENING_AGENT: {
                "mode": "primary",
                "description": "Cleans up one executed plan.",
                "permissions": evergreening_permissions(),
            },
            REBASING_AGENT: {
                "mode": "primary",
                "description": "Rebases one plan branch onto its target branch.",
                "permissions": rebasing_permissions(),
            },
        },
    });
    root["instructions"] = serde_json::json!([instructions.to_string_lossy().to_string()]);
    root.to_string()
}

/// Deterministic temp path for one session's instructions file. Pure.
pub fn instructions_path(window: &str, key: &SessionKey) -> PathBuf {
    let role = match key.role {
        SessionRole::Scoping => "scoping",
        SessionRole::Executing => "executing",
        SessionRole::Evergreening => "evergreening",
        SessionRole::Rebasing => "rebasing",
    };
    std::env::temp_dir().join(format!(
        "samokod-{}-{}-{}.md",
        sanitize_filename_component(window),
        sanitize_filename_component(&key.plan),
        role
    ))
}

/// Write role instructions for OpenCode's `instructions` source. Best
/// effort: logs on failure and keeps the session.
pub fn write_instructions_file(path: &Path, system: &str) {
    if let Err(error) = std::fs::write(path, system) {
        log::warn!(
            "failed to write instructions file {}: {error}",
            path.display()
        );
    }
}

/// Best-effort cleanup for one session's instructions file.
pub fn remove_instructions_file(window: &str, key: &SessionKey) {
    let path = instructions_path(window, key);
    if let Err(error) = std::fs::remove_file(&path)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!(
            "failed to remove instructions file {}: {error}",
            path.display()
        );
    }
}

fn sanitize_filename_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unnamed".to_string()
    } else {
        cleaned
    }
}

/// One ordered V2 permission rule. Array order is the contract: OpenCode
/// grants the last matching rule.
fn rule(action: &str, resource: &str, effect: &str) -> Value {
    serde_json::json!({
        "action": action,
        "resource": resource,
        "effect": effect,
    })
}

/// Ordered V2 edit rules from `(resource, effect)` pairs.
fn edit_rules(rules: &[(&str, &str)]) -> Vec<Value> {
    rules
        .iter()
        .map(|(resource, effect)| rule("edit", resource, effect))
        .collect()
}

/// Scoping edits stay inside its plan scope, except state and
/// attachments; the shell stays fully allowed for investigation.
fn scoping_permissions(plan: &PlanRef) -> Vec<Value> {
    let scope = plan.scope_glob();
    let mut permissions = vec![
        rule("read", "*", "allow"),
        rule("external_directory", "*", "allow"),
    ];
    permissions.extend(edit_rules(&[
        ("*", "deny"),
        (scope.as_str(), "allow"),
        (STATE_DENY_GLOB, "deny"),
        (ATTACHMENTS_DENY_GLOB, "deny"),
    ]));
    permissions.push(rule("shell", "*", "allow"));
    permissions.push(rule("question", "*", "deny"));
    permissions.extend(
        SCOPING_SUBAGENT_RULES
            .iter()
            .map(|(resource, effect)| rule("subagent", resource, effect)),
    );
    permissions
}

/// Scoping may only spawn the read-only researcher. Denied agents vanish
/// from the subagent tool description, so the model will not attempt them.
/// V2 has no built-in `scout`, so only `explore` is allowed.
const SCOPING_SUBAGENT_RULES: [(&str, &str); 2] = [("*", "deny"), ("explore", "allow")];

/// Executing runs an approved plan, so the shell is fully allowed. Edits
/// ask inside `.samokod` to stop casual rewrites of app state through the
/// obvious path; shell bypass is accepted. The bare `.samokod` key covers
/// the directory entry itself, since `.samokod/**` only matches paths
/// starting with `.samokod/`, and state plus attachments deny.
fn executing_permissions() -> Vec<Value> {
    let mut permissions = vec![
        rule("read", "*", "allow"),
        rule("external_directory", "*", "allow"),
    ];
    permissions.extend(edit_rules(&WORK_EDIT_RULES));
    permissions.push(rule("shell", "*", "allow"));
    permissions.push(rule("question", "*", "deny"));
    permissions
}

/// Evergreening runs in the same worktree as executing, so it shares the
/// work edit shape: everything editable except state and attachments, with
/// the `.samokod` ask guard and no permission questions.
fn evergreening_permissions() -> Vec<Value> {
    let mut permissions = vec![
        rule("read", "*", "allow"),
        rule("external_directory", "*", "allow"),
    ];
    permissions.extend(edit_rules(&WORK_EDIT_RULES));
    permissions.push(rule("shell", "*", "allow"));
    permissions.push(rule("question", "*", "deny"));
    permissions
}

/// Shared executing and evergreening edit shape. Denies land last so deny
/// wins.
const WORK_EDIT_RULES: [(&str, &str); 5] = [
    ("*", "allow"),
    (".samokod", "ask"),
    (".samokod/**", "ask"),
    (STATE_DENY_GLOB, "deny"),
    (ATTACHMENTS_DENY_GLOB, "deny"),
];

/// Rebasing resolves one plan branch onto its target, so every worktree
/// file is editable except state and attachments: a rebased commit must
/// cover files the target branch added on top. No permission questions
/// and no subagent research: rebasing is a focused conflict-resolution
/// task.
fn rebasing_permissions() -> Vec<Value> {
    let mut permissions = vec![
        rule("read", "*", "allow"),
        rule("external_directory", "*", "allow"),
    ];
    permissions.extend(edit_rules(&REBASING_EDIT_RULES));
    permissions.push(rule("shell", "*", "allow"));
    permissions.push(rule("question", "*", "deny"));
    permissions.push(rule("subagent", "*", "deny"));
    permissions
}

const REBASING_EDIT_RULES: [(&str, &str); 3] = [
    ("*", "allow"),
    (STATE_DENY_GLOB, "deny"),
    (ATTACHMENTS_DENY_GLOB, "deny"),
];

/// Plan dir as the agent sees it: repo-relative, forward slashes. Pure.
pub fn plan_display(plan: &PlanRef) -> String {
    format!(".samokod/plans/{}/{}", plan.phase.dir_name(), plan.name)
}

/// Scoping instructions with the plan dir filled in. System text for
/// `session/new`, never rendered. Pure.
pub fn scoping_system(plan_dir: &str) -> String {
    SCOPING_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

/// Executing role and instructions. System text for `session/new`. Pure.
pub fn executing_system(plan_dir: &str) -> String {
    EXECUTING_PROMPT.replace("{{PLAN_DIR}}", plan_dir)
}

/// Evergreening role and instructions. System text for `session/new`. Pure.
pub fn evergreening_system(
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

/// Rebasing role and instructions. System text for `session/new`. Pure.
pub fn rebasing_system(
    worktree_branch: &str,
    target_branch: &str,
    worktree_path: &str,
    plan_md_abs_path: &str,
) -> String {
    REBASING_PROMPT
        .replace("{{WORKTREE_BRANCH}}", worktree_branch)
        .replace("{{TARGET_BRANCH}}", target_branch)
        .replace("{{WORKTREE_PATH}}", worktree_path)
        .replace("{{PLAN_MD_ABS_PATH}}", plan_md_abs_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plans::{Phase, STATE_FILE};

    fn test_plan() -> PlanRef {
        PlanRef {
            name: "ts".to_string(),
            phase: Phase::Scoping,
        }
    }

    fn config(plan: &PlanRef) -> Value {
        serde_json::from_str(&agent_config(plan, Path::new("/tmp/samokod-test.md")))
            .expect("valid JSON")
    }

    /// Mirror of OpenCode's find-last wildcard match: the last rule whose
    /// action and resource match wins. Native V2 `permissions` array shape.
    fn effect_of(config: &Value, agent: &str, action: &str, resource: &str) -> String {
        let permissions = config["agents"][agent]["permissions"]
            .as_array()
            .expect("agent permissions is an array");
        let mut effect = "none";
        for entry in permissions {
            let entry_action = entry["action"].as_str().expect("rule action is a string");
            if entry_action != "*" && entry_action != action {
                continue;
            }
            let pattern = entry["resource"]
                .as_str()
                .expect("rule resource is a string");
            if wildcard_match(pattern, resource) {
                effect = entry["effect"].as_str().expect("rule effect is a string");
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

    fn resources_of(config: &Value, agent: &str, action: &str) -> Vec<String> {
        config["agents"][agent]["permissions"]
            .as_array()
            .expect("agent permissions is an array")
            .iter()
            .filter(|entry| entry["action"].as_str() == Some(action))
            .map(|entry| {
                entry["resource"]
                    .as_str()
                    .expect("rule resource is a string")
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn todos_are_enabled() {
        let config = config(&test_plan());
        assert_eq!(config["experimental"]["todo"], true);
    }

    #[test]
    fn all_agents_are_primary_without_system_field() {
        let config = config(&test_plan());
        for agent in [
            SCOPING_AGENT,
            EXECUTING_AGENT,
            EVERGREENING_AGENT,
            REBASING_AGENT,
        ] {
            let entry = config
                .pointer(&format!("/agents/{agent}"))
                .expect("agent present");
            assert_eq!(entry["mode"], "primary");
            assert!(entry.get("system").is_none(), "{agent} must not set system");
            assert!(
                entry.get("prompt").is_none(),
                "{agent} must not set legacy prompt"
            );
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
    fn scoping_allows_shell() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, SCOPING_AGENT, "shell", "cargo test"),
            "allow"
        );
    }

    #[test]
    fn scoping_subagent_allows_only_explore() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, SCOPING_AGENT, "subagent", "explore"),
            "allow",
            "explore should be allowed"
        );
        for agent_type in ["general", "scout", "unknown-type"] {
            assert_eq!(
                effect_of(&config, SCOPING_AGENT, "subagent", agent_type),
                "deny",
                "{agent_type} should be denied"
            );
        }
    }

    #[test]
    fn executing_allows_edits_and_shell() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, EXECUTING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, EXECUTING_AGENT, "shell", "cargo test"),
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
            resources_of(&config, SCOPING_AGENT, "edit"),
            vec![
                "*".to_string(),
                plan.scope_glob(),
                STATE_DENY_GLOB.to_string(),
                ATTACHMENTS_DENY_GLOB.to_string(),
            ]
        );
        assert_eq!(
            resources_of(&config, SCOPING_AGENT, "subagent"),
            vec!["*".to_string(), "explore".to_string()]
        );
        assert_eq!(
            resources_of(&config, EXECUTING_AGENT, "edit"),
            vec![
                "*".to_string(),
                ".samokod".to_string(),
                ".samokod/**".to_string(),
                STATE_DENY_GLOB.to_string(),
                ATTACHMENTS_DENY_GLOB.to_string(),
            ]
        );
        assert_eq!(
            resources_of(&config, EVERGREENING_AGENT, "edit"),
            vec![
                "*".to_string(),
                ".samokod".to_string(),
                ".samokod/**".to_string(),
                STATE_DENY_GLOB.to_string(),
                ATTACHMENTS_DENY_GLOB.to_string(),
            ]
        );
        assert_eq!(
            resources_of(&config, REBASING_AGENT, "edit"),
            vec![
                "*".to_string(),
                STATE_DENY_GLOB.to_string(),
                ATTACHMENTS_DENY_GLOB.to_string(),
            ]
        );
    }

    #[test]
    fn scoping_and_executing_deny_question() {
        let config = config(&test_plan());
        for agent in [
            SCOPING_AGENT,
            EXECUTING_AGENT,
            EVERGREENING_AGENT,
            REBASING_AGENT,
        ] {
            assert_eq!(
                effect_of(&config, agent, "question", "*"),
                "deny",
                "{agent} should deny questions"
            );
        }
    }

    #[test]
    fn rebasing_allows_worktree_edits_without_subagents() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, REBASING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, REBASING_AGENT, "edit", ".samokod/state.json"),
            "allow"
        );
        assert_eq!(
            effect_of(
                &config,
                REBASING_AGENT,
                "edit",
                &format!(".samokod/plans/rebasing/x/{STATE_FILE}")
            ),
            "deny"
        );
        assert_eq!(
            effect_of(&config, REBASING_AGENT, "shell", "git rebase -i feature"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, REBASING_AGENT, "subagent", "explore"),
            "deny"
        );
    }

    #[test]
    fn rebasing_plans_run_the_rebasing_agent() {
        assert_eq!(agent_for(SessionRole::Rebasing), REBASING_AGENT);
        assert_eq!(agent_for(SessionRole::Evergreening), EVERGREENING_AGENT);
        assert_eq!(agent_for(SessionRole::Executing), EXECUTING_AGENT);
        assert_eq!(agent_for(SessionRole::Scoping), SCOPING_AGENT);
    }

    #[test]
    fn evergreening_allows_edits_and_shell() {
        let config = config(&test_plan());
        assert_eq!(
            effect_of(&config, EVERGREENING_AGENT, "edit", "src/App.tsx"),
            "allow"
        );
        assert_eq!(
            effect_of(&config, EVERGREENING_AGENT, "shell", "cargo test"),
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
                effect_of(&config, REBASING_AGENT, "edit", &resource),
                "deny",
                "{resource} should deny for rebasing"
            );
        }
    }

    #[test]
    fn attachments_deny_last_for_every_agent() {
        let plan = test_plan();
        let config = config(&plan);
        let own = format!(
            ".samokod/plans/scoping/{}/attachments/image-1-0.png",
            plan.name
        );
        assert_eq!(effect_of(&config, SCOPING_AGENT, "edit", &own), "deny");
        for resource in [
            ".samokod/plans/scoping/other/attachments/image-1-0.png".to_string(),
            ".samokod/plans/executing/x/attachments/image-1-0.jpg".to_string(),
            ".samokod/plans/evergreening/x/attachments/image-1-0.webp".to_string(),
            ".samokod/plans/rebasing/x/attachments/image-1-0.gif".to_string(),
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
                effect_of(&config, REBASING_AGENT, "edit", &resource),
                "deny",
                "{resource} should deny for rebasing"
            );
        }
    }

    #[test]
    fn evergreening_system_fills_every_placeholder() {
        let system = evergreening_system(
            "/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature",
            "samokod/shiny-feature",
            "/repo/.samokod/plans/evergreening/2026-09-26.14-53-26.shiny-feature/plan.md",
            "feature",
            "0923748 feat: blabla\n009fb12 fix: blabla",
        );
        assert!(system.contains("/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature"));
        assert!(system.contains("samokod/shiny-feature"));
        assert!(system.contains(
            "/repo/.samokod/plans/evergreening/2026-09-26.14-53-26.shiny-feature/plan.md"
        ));
        assert!(system.contains("feature"));
        assert!(system.contains("0923748 feat: blabla"));
        assert!(system.contains("009fb12 fix: blabla"));
        assert!(!system.contains("{{WORKTREE_PATH}}"));
        assert!(!system.contains("{{WORKTREE_BRANCH}}"));
        assert!(!system.contains("{{PLAN_MD_ABS_PATH}}"));
        assert!(!system.contains("{{TARGET_BRANCH}}"));
        assert!(!system.contains("{{COMMITS}}"));
    }

    #[test]
    fn rebasing_system_fills_every_placeholder() {
        let system = rebasing_system(
            "samokod/shiny-feature",
            "feature",
            "/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature",
            "/repo/.samokod/plans/rebasing/2026-09-26.14-53-26.shiny-feature/plan.md",
        );
        assert!(system.contains("samokod/shiny-feature"));
        assert!(system.contains("`feature`"));
        assert!(system.contains("/repo/.samokod/worktrees/2026-09-26.14-53-26.shiny-feature"));
        assert!(!system.contains("{{WORKTREE_BRANCH}}"));
        assert!(!system.contains("{{TARGET_BRANCH}}"));
        assert!(!system.contains("{{WORKTREE_PATH}}"));
        assert!(!system.contains("{{PLAN_MD_ABS_PATH}}"));
    }

    #[test]
    fn scoping_system_names_plan_path() {
        let system = scoping_system(".samokod/plans/scoping/ts");
        assert!(system.contains(".samokod/plans/scoping/ts/plan.md"));
        assert!(!system.contains("{{PLAN_DIR}}"));
    }

    #[test]
    fn executing_system_names_plan_path() {
        let system = executing_system(".samokod/plans/executing/ts.slug");
        assert!(system.contains(".samokod/plans/executing/ts.slug/plan.md"));
        assert!(!system.contains("{{PLAN_DIR}}"));
    }

    #[test]
    fn scoping_system_enforces_commit_per_step() {
        let system = scoping_system(".samokod/plans/scoping/ts");
        assert!(system.contains("scoping agent"));
        assert!(system.contains("starts with the word Commit"));
        assert!(system.contains("(non-commit step)"));
        assert!(system.contains("never be broken"));
    }

    #[test]
    fn scoping_system_resolves_unknowns_without_manual_steps() {
        let system = scoping_system(".samokod/plans/scoping/ts");
        assert!(system.contains("Run every experiment"));
        assert!(system.contains("no spikes"));
        assert!(system.contains("what must hold"));
        assert!(system.contains("visually confirm"));
        assert!(system.contains("automated proxy"));
    }

    #[test]
    fn executing_system_commits_per_step_with_clean_tree() {
        let system = executing_system(".samokod/plans/executing/ts.slug");
        assert!(system.contains("executing agent"));
        assert!(system.contains("commit immediately"));
        assert!(system.contains("Never batch"));
        assert!(system.contains("git status --short"));
        assert!(system.contains("must come back clean"));
        assert!(system.contains("no commit stays red"));
    }

    #[test]
    fn all_systems_leave_no_placeholders() {
        let dir = ".samokod/plans/scoping/ts";
        let systems = [
            scoping_system(dir),
            executing_system(dir),
            rebasing_system(
                "samokod/ts",
                "main",
                "/repo/worktree",
                "/repo/.samokod/plans/rebasing/ts/plan.md",
            ),
            evergreening_system(
                "/repo/worktree",
                "samokod/ts",
                "/repo/.samokod/plans/evergreening/ts/plan.md",
                "main",
                "abc feat: x",
            ),
        ];
        for system in &systems {
            assert!(!system.contains("{{"));
            assert!(!system.contains("}}"));
        }
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

    #[test]
    fn instructions_path_is_deterministic_and_sanitized() {
        let key = SessionKey {
            plan: "2026-09-26.ts".to_string(),
            role: SessionRole::Scoping,
        };
        let first = instructions_path("main", &key);
        let second = instructions_path("main", &key);
        assert_eq!(first, second);
        assert!(first.is_absolute());
        assert_eq!(first.extension().and_then(|ext| ext.to_str()), Some("md"));
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some("samokod-main-2026-09-26_ts-scoping.md")
        );
        let evil = SessionKey {
            plan: "../evil".to_string(),
            role: SessionRole::Executing,
        };
        let path = instructions_path("win/dow", &evil);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("filename");
        assert_eq!(name, "samokod-win_dow-___evil-executing.md");
        assert!(path.is_absolute());
    }

    #[test]
    fn instructions_config_carries_session_path() {
        let plan = test_plan();
        let abs = std::env::temp_dir().join("samokod-test-instructions.md");
        let parsed: Value = serde_json::from_str(&agent_config(&plan, &abs)).expect("valid JSON");
        assert_eq!(
            parsed["instructions"],
            serde_json::json!([abs.to_string_lossy().to_string()])
        );
        let env = agent_env(&plan, &abs);
        let content = env.get(CONFIG_CONTENT_ENV).expect("config content");
        let parsed: Value = serde_json::from_str(content).expect("valid JSON");
        assert_eq!(
            parsed["instructions"],
            serde_json::json!([abs.to_string_lossy().to_string()])
        );
    }

    #[test]
    fn session_env_writes_file_before_returning() {
        let key = SessionKey {
            plan: "2026-09-26.ts".to_string(),
            role: SessionRole::Scoping,
        };
        let plan = test_plan();
        let system = scoping_system(".samokod/plans/scoping/ts");
        let env = session_env("main", &key, &plan, &system);
        let path = instructions_path("main", &key);
        let back = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(back, system);
        let content = env.get(CONFIG_CONTENT_ENV).expect("config content");
        let parsed: Value = serde_json::from_str(content).expect("valid JSON");
        assert_eq!(
            parsed["instructions"],
            serde_json::json!([path.to_string_lossy().to_string()])
        );
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn instructions_file_roundtrips_system_text() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("role.md");
        let system = scoping_system(".samokod/plans/scoping/ts");
        write_instructions_file(&path, &system);
        let back = std::fs::read_to_string(&path).expect("read back");
        assert_eq!(back, system);
        assert!(!back.contains("{{PLAN_DIR}}"));
    }
}
