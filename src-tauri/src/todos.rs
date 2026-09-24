// Todo list read off tool payloads by shape.
use crate::types::{TodoChangeView, TodoView};

/// Extract a todo list from a `tool_call` raw input, if it carries one.
pub fn todos_from_call(raw_input: Option<&serde_json::Value>) -> Option<Vec<TodoView>> {
    raw_input.and_then(parse_todos)
}

/// Extract a todo list from a `tool_call_update`, preferring the raw output
/// over the raw input, if either carries one.
pub fn todos_from_update(
    raw_input: Option<&serde_json::Value>,
    raw_output: Option<&serde_json::Value>,
) -> Option<Vec<TodoView>> {
    raw_output
        .and_then(parse_todos)
        .or_else(|| raw_input.and_then(parse_todos))
}

/// Parse a todo list from a raw tool payload. Both shapes are accepted: the
/// call input (`{todos: [...]}`) and the completed output (`{metadata:
/// {todos: [...]}}`). Returns `None` when no list is present so callers can
/// tell "not a todo payload" apart from "cleared list". Rows missing a field
/// are skipped.
fn parse_todos(payload: &serde_json::Value) -> Option<Vec<TodoView>> {
    let list = payload
        .get("todos")
        .or_else(|| payload.get("metadata").and_then(|meta| meta.get("todos")))?
        .as_array()?;
    Some(
        list.iter()
            .filter_map(|item| {
                Some(TodoView {
                    content: item.get("content")?.as_str()?.to_string(),
                    status: item.get("status")?.as_str()?.to_string(),
                    priority: item.get("priority")?.as_str()?.to_string(),
                })
            })
            .collect(),
    )
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

    fn todo_items() -> serde_json::Value {
        json!([
            {"content": "a", "status": "pending", "priority": "medium"},
            {"content": "b", "status": "pending", "priority": "medium"},
        ])
    }

    #[test]
    fn parses_call_input_shape() {
        let payload = json!({"todos": [
            {"content": "a", "status": "pending", "priority": "high"},
            {"content": "b", "status": "in_progress", "priority": "low"},
        ]});
        let todos = parse_todos(&payload).expect("todos key present");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[1].status, "in_progress");
    }

    #[test]
    fn parses_completed_output_shape() {
        let payload = json!({"output": "…", "metadata": {"todos": [
            {"content": "a", "status": "completed", "priority": "high"},
        ], "truncated": false}});
        let todos = parse_todos(&payload).expect("metadata.todos present");
        assert_eq!(todos, vec![todo("a", "completed")]);
    }

    #[test]
    fn missing_key_returns_none() {
        assert!(parse_todos(&json!({})).is_none());
        assert!(parse_todos(&json!({"output": "…", "metadata": {}})).is_none());
    }

    #[test]
    fn empty_list_returns_some_empty() {
        assert_eq!(parse_todos(&json!({"todos": []})), Some(Vec::new()));
    }

    #[test]
    fn skips_rows_with_missing_fields() {
        let payload = json!({"todos": [
            {"content": "a", "status": "pending"},
            {"content": "b", "status": "pending", "priority": "low"},
        ]});
        let todos = parse_todos(&payload).expect("todos key present");
        assert_eq!(todos.len(), 1);
    }

    #[test]
    fn call_extracts_input() {
        let todos = todo_items();
        let raw = json!({"todos": todos});
        let fresh = todos_from_call(Some(&raw)).expect("input carries todos");
        assert_eq!(fresh.len(), 2);
        assert_eq!(fresh[0].content, "a");
    }

    #[test]
    fn call_ignores_empty_input() {
        assert!(todos_from_call(None).is_none());
        assert!(todos_from_call(Some(&json!({}))).is_none());
    }

    #[test]
    fn update_prefers_output_over_input() {
        let input = json!({"todos": [{"content": "a", "status": "pending", "priority": "high"}]});
        let output =
            json!({"output": "…", "metadata": {"todos": todo_items(), "truncated": false}});
        let fresh = todos_from_update(Some(&input), Some(&output)).expect("output carries todos");
        assert_eq!(fresh.len(), 2);
        assert_eq!(fresh[0].content, "a");
    }

    #[test]
    fn update_falls_back_to_input() {
        let input = json!({"todos": todo_items()});
        let fresh = todos_from_update(Some(&input), None).expect("input carries todos");
        assert_eq!(fresh.len(), 2);
    }

    #[test]
    fn update_ignores_payloads_without_todos() {
        assert!(todos_from_update(None, None).is_none());
        assert!(todos_from_update(Some(&json!({})), Some(&json!({"output": "…"}))).is_none());
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
