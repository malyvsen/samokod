// Permission card content. Only one-shot options reach the frontend so every
// session stays explicit.
use crate::acp::{PermissionOption, PermissionOptionKind};
use crate::types::PermissionOptionView;

/// Convert wire options to the frontend view, dropping `always` variants.
/// Pure.
pub fn to_view(options: &[PermissionOption]) -> Vec<PermissionOptionView> {
    options
        .iter()
        .filter_map(|option| {
            let kind = match option.kind {
                PermissionOptionKind::AllowOnce => "allow",
                PermissionOptionKind::RejectOnce => "reject",
                _ => return None,
            };
            Some(PermissionOptionView {
                id: option.option_id.to_string(),
                kind: kind.to_string(),
            })
        })
        .collect()
}

/// Effective-rule copy. States the rule only, never the config file it came
/// from, because ACP does not carry that information. Pure.
pub fn rule_hint(tool_name: Option<&str>) -> String {
    match tool_name {
        Some(name) if !name.is_empty() => {
            format!("{name} - asked because your OpenCode config sets {name} to ask")
        }
        _ => "asked because your OpenCode config sets this tool to ask".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::PermissionOptionId;

    fn option(id: &str, kind: PermissionOptionKind) -> PermissionOption {
        PermissionOption::new(PermissionOptionId::new(id), id, kind)
    }

    #[test]
    fn keeps_only_one_shot_options() {
        let options = vec![
            option("a-once", PermissionOptionKind::AllowOnce),
            option("a-always", PermissionOptionKind::AllowAlways),
            option("r-once", PermissionOptionKind::RejectOnce),
            option("r-always", PermissionOptionKind::RejectAlways),
        ];
        let view = to_view(&options);
        assert_eq!(
            view.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            vec!["a-once", "r-once"]
        );
        assert_eq!(
            view.iter()
                .map(|item| item.kind.as_str())
                .collect::<Vec<_>>(),
            vec!["allow", "reject"]
        );
    }

    #[test]
    fn empty_when_only_always() {
        let options = vec![option("a-always", PermissionOptionKind::AllowAlways)];
        assert!(to_view(&options).is_empty());
    }

    #[test]
    fn rule_hint_names_tool() {
        assert_eq!(
            rule_hint(Some("bash")),
            "bash - asked because your OpenCode config sets bash to ask"
        );
        assert_eq!(
            rule_hint(None),
            "asked because your OpenCode config sets this tool to ask"
        );
    }
}
