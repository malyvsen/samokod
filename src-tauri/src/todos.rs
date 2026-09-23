// Todo list derived from `todowrite` tool payloads.
use crate::types::{TodoChangeView, TodoView};

/// Case-insensitive `todowrite` match on the programmatic tool name.
pub fn is_todowrite(name: Option<&str>) -> bool {
    name.is_some_and(|name| name.eq_ignore_ascii_case("todowrite"))
}

/// Parse a todo list from a raw tool payload. Both shapes are accepted: the
/// call input (`{todos: [...]}`) and the completed output (`{metadata:
/// {todos: [...]}}`). Rows missing a field are skipped.
pub fn parse_todos(payload: &serde_json::Value) -> Vec<TodoView> {
    let Some(list) = payload
        .get("todos")
        .or_else(|| payload.get("metadata").and_then(|meta| meta.get("todos")))
        .and_then(|todos| todos.as_array())
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|item| {
            Some(TodoView {
                content: item.get("content")?.as_str()?.to_string(),
                status: item.get("status")?.as_str()?.to_string(),
                priority: item.get("priority")?.as_str()?.to_string(),
            })
        })
        .collect()
}

/// Diff a fresh list against the previous one: added rows plus rows whose
/// status changed, matched by content. Removed rows never surface. Pure.
pub fn diff_todos(old: &[TodoView], new: &[TodoView]) -> Vec<TodoChangeView> {
    new.iter()
        .filter(|item| {
            old.iter()
                .find(|prev| prev.content == item.content)
                .is_none_or(|prev| prev.status != item.status)
        })
        .map(|item| TodoChangeView {
            content: item.content.clone(),
            status: item.status.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn todo(content: &str, status: &str) -> TodoView {
        TodoView {
            content: content.to_string(),
            status: status.to_string(),
            priority: "high".to_string(),
        }
    }

    #[test]
    fn todowrite_name_matches_case_insensitively() {
        assert!(is_todowrite(Some("todowrite")));
        assert!(is_todowrite(Some("TodoWrite")));
        assert!(!is_todowrite(Some("bash")));
        assert!(!is_todowrite(None));
    }

    #[test]
    fn parses_call_input_shape() {
        let payload = json!({"todos": [
            {"content": "a", "status": "pending", "priority": "high"},
            {"content": "b", "status": "in_progress", "priority": "low"},
        ]});
        let todos = parse_todos(&payload);
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[1].status, "in_progress");
    }

    #[test]
    fn parses_completed_output_shape() {
        let payload = json!({"output": "…", "metadata": {"todos": [
            {"content": "a", "status": "completed", "priority": "high"},
        ]}});
        let todos = parse_todos(&payload);
        assert_eq!(todos, vec![todo("a", "completed")]);
    }

    #[test]
    fn skips_rows_with_missing_fields() {
        let payload = json!({"todos": [
            {"content": "a", "status": "pending"},
            {"content": "b", "status": "pending", "priority": "low"},
        ]});
        assert_eq!(parse_todos(&payload).len(), 1);
    }

    #[test]
    fn empty_list_parses_to_empty() {
        assert!(parse_todos(&json!({"todos": []})).is_empty());
        assert!(parse_todos(&json!({})).is_empty());
    }

    #[test]
    fn diff_reports_added_and_status_changed_only() {
        let old = vec![
            todo("a", "pending"),
            todo("gone", "pending"),
            todo("same", "pending"),
        ];
        let new = vec![
            todo("a", "completed"),
            todo("b", "pending"),
            todo("same", "pending"),
        ];
        let changes = diff_todos(&old, &new);
        assert_eq!(changes.len(), 2);
        assert!(
            changes
                .iter()
                .any(|c| c.content == "a" && c.status == "completed")
        );
        assert!(changes.iter().any(|c| c.content == "b"));
    }

    #[test]
    fn identical_lists_have_no_changes() {
        let list = vec![todo("a", "pending")];
        assert!(diff_todos(&list, &list).is_empty());
    }
}
