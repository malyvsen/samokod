// Pure mappings for ACP session updates and tool calls. Every tool
// execution becomes one `▸` status line regardless of kind.
use crate::acp::{ContentBlock, SessionUpdate, ToolCall, ToolCallStatus, ToolCallUpdate, ToolKind};
use crate::types::ToolLineView;

/// Extract streamed agent text from an update, if any. Pure.
pub fn agent_text_of(update: &SessionUpdate) -> Option<String> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
            ContentBlock::Text(text) => Some(text.text.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// Short label for a session update. `None` means the protocol sent a variant
/// this client never enumerated. Pure.
pub fn update_kind(update: &SessionUpdate) -> Option<&'static str> {
    match update {
        SessionUpdate::UserMessageChunk(_) => Some("user_message_chunk"),
        SessionUpdate::AgentMessageChunk(_) => Some("agent_message_chunk"),
        SessionUpdate::AgentThoughtChunk(_) => Some("agent_thought_chunk"),
        SessionUpdate::ToolCall(_) => Some("tool_call"),
        SessionUpdate::ToolCallUpdate(_) => Some("tool_call_update"),
        SessionUpdate::Plan(_) => Some("plan"),
        SessionUpdate::AvailableCommandsUpdate(_) => Some("available_commands"),
        SessionUpdate::CurrentModeUpdate(_) => Some("current_mode"),
        SessionUpdate::ConfigOptionUpdate(_) => Some("config_option"),
        SessionUpdate::SessionInfoUpdate(_) => Some("session_info"),
        SessionUpdate::UsageUpdate(_) => Some("usage"),
        _ => None,
    }
}

/// Format one tool line. Rows stay single-line ellipsis via CSS; the hover
/// tooltip reads the same text. The title is agent text and renders verbatim;
/// ongoing statuses append ` …`, finished ones render the bare title. Pure.
pub fn format_tool_line(call: &ToolCall) -> ToolLineView {
    ToolLineView {
        id: call.tool_call_id.to_string(),
        text: tool_text(call),
        status: tool_status_label(call.status),
    }
}

/// Format a tool update line from the update title when present. Same verbatim
/// title plus ` …` rule as tool lines; empty titles fall back to name/kind.
/// Pure.
pub fn format_tool_update(update: &ToolCallUpdate) -> ToolLineView {
    let status = update.fields.status.unwrap_or_default();
    let title = update.fields.title.as_deref().map(str::trim).unwrap_or("");
    let base = if title.is_empty() {
        fallback_label(update.fields.name.as_deref(), update.fields.kind)
    } else {
        title.to_string()
    };
    ToolLineView {
        id: update.tool_call_id.to_string(),
        text: with_ongoing_suffix(&base, is_ongoing(status)),
        status: tool_status_label(status),
    }
}

fn tool_text(call: &ToolCall) -> String {
    let title = call.title.trim();
    if title.is_empty() {
        let base = fallback_label(call.name.as_deref(), Some(call.kind));
        return with_ongoing_suffix(&base, is_ongoing(call.status));
    }
    with_ongoing_suffix(title, is_ongoing(call.status))
}

fn fallback_label(name: Option<&str>, kind: Option<ToolKind>) -> String {
    match name.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => kind_label(kind).to_string(),
    }
}

fn with_ongoing_suffix(base: &str, ongoing: bool) -> String {
    if ongoing {
        format!("{base} …")
    } else {
        base.to_string()
    }
}

fn is_ongoing(status: ToolCallStatus) -> bool {
    match status {
        ToolCallStatus::Pending | ToolCallStatus::InProgress => true,
        ToolCallStatus::Completed | ToolCallStatus::Failed => false,
        _ => true,
    }
}

fn tool_status_label(status: ToolCallStatus) -> String {
    match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::InProgress => "in_progress",
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        _ => "pending",
    }
    .to_string()
}

/// Human label for a tool kind in permission cards. Pure.
pub fn kind_label(kind: Option<ToolKind>) -> &'static str {
    match kind {
        Some(ToolKind::Read) => "read",
        Some(ToolKind::Edit) => "edit",
        Some(ToolKind::Delete) => "delete",
        Some(ToolKind::Move) => "move",
        Some(ToolKind::Search) => "search",
        Some(ToolKind::Execute) => "bash",
        Some(ToolKind::Think) => "think",
        Some(ToolKind::Fetch) => "fetch",
        Some(ToolKind::SwitchMode) => "mode",
        _ => "tool",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{ContentChunk, TextContent, ToolCallStatus, ToolCallUpdateFields};

    #[test]
    fn agent_chunk_extracts_text() {
        let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("hello"),
        )));
        assert_eq!(agent_text_of(&update), Some("hello".to_string()));
    }

    #[test]
    fn non_text_update_yields_no_chunk() {
        let call = ToolCall::new("id-1", "title").kind(ToolKind::Read);
        let update = SessionUpdate::ToolCall(call);
        assert_eq!(agent_text_of(&update), None);
    }

    #[test]
    fn update_kind_labels_tool_call() {
        let call = ToolCall::new("id-1", "title").kind(ToolKind::Read);
        assert_eq!(
            update_kind(&SessionUpdate::ToolCall(call)),
            Some("tool_call")
        );
    }

    #[test]
    fn finished_tool_line_renders_title_verbatim() {
        for status in [ToolCallStatus::Completed, ToolCallStatus::Failed] {
            let call = ToolCall::new("id-1", "edit file.md")
                .kind(ToolKind::Edit)
                .status(status);
            let line = format_tool_line(&call);
            assert_eq!(line.text, "edit file.md", "{status:?}");
            assert_eq!(
                line.status,
                if status == ToolCallStatus::Completed {
                    "completed"
                } else {
                    "failed"
                }
            );
        }
    }

    #[test]
    fn ongoing_tool_line_appends_ellipsis() {
        for status in [ToolCallStatus::Pending, ToolCallStatus::InProgress] {
            let call = ToolCall::new("id-1", "edit file.md")
                .kind(ToolKind::Edit)
                .status(status);
            let line = format_tool_line(&call);
            assert_eq!(line.text, "edit file.md …", "{status:?}");
        }
    }

    #[test]
    fn status_label_keeps_in_progress_snake_case() {
        let call = ToolCall::new("id-1", "edit file.md")
            .kind(ToolKind::Edit)
            .status(ToolCallStatus::InProgress);
        assert_eq!(format_tool_line(&call).status, "in_progress");
    }

    #[test]
    fn update_line_follows_title_and_status() {
        let finished = ToolCallUpdate::new(
            "id-9",
            ToolCallUpdateFields::new()
                .title("edit file.md")
                .status(ToolCallStatus::Completed),
        );
        assert_eq!(format_tool_update(&finished).text, "edit file.md");

        let ongoing = ToolCallUpdate::new(
            "id-9",
            ToolCallUpdateFields::new()
                .title("edit file.md")
                .status(ToolCallStatus::InProgress),
        );
        assert_eq!(format_tool_update(&ongoing).text, "edit file.md …");

        let default_status =
            ToolCallUpdate::new("id-9", ToolCallUpdateFields::new().title("edit file.md"));
        assert_eq!(format_tool_update(&default_status).text, "edit file.md …");
    }

    #[test]
    fn empty_title_prefers_name_then_kind() {
        let mut call = ToolCall::new("id-1", "  ").kind(ToolKind::Execute);
        call.name = Some("edit".to_string());
        call.status = ToolCallStatus::Completed;
        assert_eq!(format_tool_line(&call).text, "edit");

        call.status = ToolCallStatus::Pending;
        assert_eq!(format_tool_line(&call).text, "edit …");

        let update = ToolCallUpdate::new(
            "id-9",
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .name("bash"),
        );
        assert_eq!(format_tool_update(&update).text, "bash");
    }

    #[test]
    fn every_tool_kind_falls_back_without_ran() {
        for kind in [
            ToolKind::Read,
            ToolKind::Edit,
            ToolKind::Delete,
            ToolKind::Move,
            ToolKind::Search,
            ToolKind::Execute,
            ToolKind::Think,
            ToolKind::Fetch,
            ToolKind::SwitchMode,
            ToolKind::Other,
        ] {
            let expected = kind_label(Some(kind));
            let call = ToolCall::new("id-1", "")
                .kind(kind)
                .status(ToolCallStatus::Completed);
            let line = format_tool_line(&call);
            assert_eq!(line.text, expected, "{kind:?}");
            assert!(!line.text.contains("ran"), "{kind:?}");

            let ongoing = ToolCall::new("id-1", "")
                .kind(kind)
                .status(ToolCallStatus::Pending);
            assert_eq!(
                format_tool_line(&ongoing).text,
                format!("{expected} …"),
                "{kind:?}"
            );

            let update = ToolCallUpdate::new(
                "id-9",
                ToolCallUpdateFields::new()
                    .kind(kind)
                    .status(ToolCallStatus::Completed),
            );
            let line = format_tool_update(&update);
            assert_eq!(line.text, expected, "{kind:?}");
            assert_ne!(line.text, "running tool", "{kind:?}");
        }
    }
}
