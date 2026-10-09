// Todo list read off tool payloads by shape.
use crate::types::{TodoChangeView, TodoStatus, TodoView};

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
/// or carrying an unknown status are skipped.
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
                    status: parse_status(item.get("status")?.as_str()?)?,
                })
            })
            .collect(),
    )
}

fn parse_status(raw: &str) -> Option<TodoStatus> {
    match raw {
        "pending" => Some(TodoStatus::Pending),
        "in_progress" => Some(TodoStatus::InProgress),
        "completed" => Some(TodoStatus::Completed),
        _ => None,
    }
}

/// Remaining-time estimate from the completion rate: elapsed per completed
/// todo times the todos left. `None` for empty, all-done, and
/// nothing-completed-yet (no timing data). Pure.
pub fn estimate_remaining(
    done: usize,
    total: usize,
    started: std::time::SystemTime,
    now: std::time::SystemTime,
) -> Option<std::time::Duration> {
    if total == 0 || done == 0 || done >= total {
        return None;
    }
    let elapsed = now.duration_since(started).ok()?;
    let remaining = (total - done) as u32;
    elapsed.checked_div(done as u32)?.checked_mul(remaining)
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
            status: item.status,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn todo(content: &str, status: TodoStatus) -> TodoView {
        TodoView {
            content: content.to_string(),
            status,
        }
    }

    fn todo_items() -> serde_json::Value {
        json!([
            {"content": "a", "status": "pending"},
            {"content": "b", "status": "pending"},
        ])
    }

    #[test]
    fn parses_call_input_shape() {
        let payload = json!({"todos": [
            {"content": "a", "status": "pending"},
            {"content": "b", "status": "in_progress"},
        ]});
        let todos = parse_todos(&payload).expect("todos key present");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[1].status, TodoStatus::InProgress);
    }

    #[test]
    fn parses_completed_output_shape() {
        let payload = json!({"output": "…", "metadata": {"todos": [
            {"content": "a", "status": "completed"},
        ], "truncated": false}});
        let todos = parse_todos(&payload).expect("metadata.todos present");
        assert_eq!(todos, vec![todo("a", TodoStatus::Completed)]);
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
            {"content": "a"},
            {"content": "b", "status": "pending"},
        ]});
        let todos = parse_todos(&payload).expect("todos key present");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "b");
    }

    #[test]
    fn skips_rows_with_unknown_status() {
        let payload = json!({"todos": [
            {"content": "a", "status": "cancelled"},
            {"content": "b", "status": "pending"},
        ]});
        let todos = parse_todos(&payload).expect("todos key present");
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].content, "b");
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
        let input = json!({"todos": [{"content": "a", "status": "pending"}]});
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
            todo("a", TodoStatus::Pending),
            todo("gone", TodoStatus::Pending),
            todo("same", TodoStatus::Pending),
        ];
        let new = vec![
            todo("a", TodoStatus::Completed),
            todo("b", TodoStatus::Pending),
            todo("same", TodoStatus::Pending),
        ];
        let changes = diff_todos(&old, &new);
        assert_eq!(changes.len(), 2);
        assert!(
            changes
                .iter()
                .any(|c| c.content == "a" && c.status == TodoStatus::Completed)
        );
        assert!(changes.iter().any(|c| c.content == "b"));
    }

    #[test]
    fn identical_lists_have_no_changes() {
        let list = vec![todo("a", TodoStatus::Pending)];
        assert!(diff_todos(&list, &list).is_empty());
    }

    #[test]
    fn estimate_empty_returns_none() {
        let now = std::time::SystemTime::now();
        assert_eq!(estimate_remaining(0, 0, now, now), None);
    }

    #[test]
    fn estimate_done_returns_none() {
        let now = std::time::SystemTime::now();
        assert_eq!(estimate_remaining(5, 5, now, now), None);
    }

    #[test]
    fn estimate_nothing_completed_returns_none() {
        let now = std::time::SystemTime::now();
        assert_eq!(estimate_remaining(0, 5, now, now), None);
    }

    #[test]
    fn estimate_partial_scales_elapsed_by_remaining() {
        let started = std::time::SystemTime::now();
        let now = started + std::time::Duration::from_secs(600);
        assert_eq!(
            estimate_remaining(2, 5, started, now),
            Some(std::time::Duration::from_secs(900))
        );
    }

    #[test]
    fn estimate_zero_done_never_divides() {
        let started = std::time::SystemTime::now();
        let now = started + std::time::Duration::from_secs(60);
        // Would divide by zero without the done == 0 guard.
        assert_eq!(estimate_remaining(0, 3, started, now), None);
    }
}
