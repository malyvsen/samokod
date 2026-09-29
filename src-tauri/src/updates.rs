// Pure mappings for ACP session updates and tool calls. Every tool
// execution becomes one `▸` status line regardless of kind.
use crate::acp::{ContentBlock, SessionUpdate, ToolCall, ToolCallStatus, ToolCallUpdate, ToolKind};
use crate::types::{ToolKindLabel, ToolLineView, ToolStatus};

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
    let status = to_tool_status(call.status);
    ToolLineView {
        id: call.tool_call_id.to_string(),
        text: tool_text(call, status),
        status,
    }
}

/// Format a tool update line from the update title when present. Same verbatim
/// title plus ` …` rule as tool lines; empty titles fall back to name/kind.
/// Pure.
pub fn format_tool_update(update: &ToolCallUpdate) -> ToolLineView {
    let status = update.fields.status.map(to_tool_status).unwrap_or_default();
    let title = update.fields.title.as_deref().map(str::trim).unwrap_or("");
    let base = if title.is_empty() {
        fallback_label(update.fields.name.as_deref(), update.fields.kind)
    } else {
        title.to_string()
    };
    ToolLineView {
        id: update.tool_call_id.to_string(),
        text: with_ongoing_suffix(&base, status),
        status,
    }
}

fn tool_text(call: &ToolCall, status: ToolStatus) -> String {
    let title = call.title.trim();
    if title.is_empty() {
        let base = fallback_label(call.name.as_deref(), Some(call.kind));
        return with_ongoing_suffix(&base, status);
    }
    with_ongoing_suffix(title, status)
}

fn fallback_label(name: Option<&str>, kind: Option<ToolKind>) -> String {
    match name.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => kind_label(kind).as_str().to_string(),
    }
}

fn with_ongoing_suffix(base: &str, status: ToolStatus) -> String {
    if status.is_ongoing() {
        format!("{base} …")
    } else {
        base.to_string()
    }
}

fn to_tool_status(status: ToolCallStatus) -> ToolStatus {
    match status {
        ToolCallStatus::Pending => ToolStatus::Pending,
        ToolCallStatus::InProgress => ToolStatus::InProgress,
        ToolCallStatus::Completed => ToolStatus::Completed,
        ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Pending,
    }
}

/// Human label for a tool kind, shared by tool lines and permission cards.
/// Pure.
pub fn kind_label(kind: Option<ToolKind>) -> ToolKindLabel {
    match kind {
        Some(ToolKind::Read) => ToolKindLabel::Read,
        Some(ToolKind::Edit) => ToolKindLabel::Edit,
        Some(ToolKind::Delete) => ToolKindLabel::Delete,
        Some(ToolKind::Move) => ToolKindLabel::Move,
        Some(ToolKind::Search) => ToolKindLabel::Search,
        Some(ToolKind::Execute) => ToolKindLabel::Execute,
        Some(ToolKind::Think) => ToolKindLabel::Think,
        Some(ToolKind::Fetch) => ToolKindLabel::Fetch,
        Some(ToolKind::SwitchMode) => ToolKindLabel::SwitchMode,
        Some(ToolKind::Other) | None => ToolKindLabel::Other,
        _ => ToolKindLabel::Other,
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
        for (wire, view) in [
            (ToolCallStatus::Completed, ToolStatus::Completed),
            (ToolCallStatus::Failed, ToolStatus::Failed),
        ] {
            let call = ToolCall::new("id-1", "edit file.md")
                .kind(ToolKind::Edit)
                .status(wire);
            let line = format_tool_line(&call);
            assert_eq!(line.text, "edit file.md", "{wire:?}");
            assert_eq!(line.status, view);
        }
    }

    #[test]
    fn ongoing_tool_line_appends_ellipsis() {
        for (wire, view) in [
            (ToolCallStatus::Pending, ToolStatus::Pending),
            (ToolCallStatus::InProgress, ToolStatus::InProgress),
        ] {
            let call = ToolCall::new("id-1", "edit file.md")
                .kind(ToolKind::Edit)
                .status(wire);
            let line = format_tool_line(&call);
            assert_eq!(line.text, "edit file.md …", "{wire:?}");
            assert_eq!(line.status, view);
        }
    }

    #[test]
    fn status_label_keeps_in_progress_snake_case() {
        let call = ToolCall::new("id-1", "edit file.md")
            .kind(ToolKind::Edit)
            .status(ToolCallStatus::InProgress);
        let line = format_tool_line(&call);
        assert_eq!(line.status, ToolStatus::InProgress);
        assert_eq!(
            serde_json::to_value(line.status).expect("status serializes"),
            serde_json::Value::String("in_progress".to_string())
        );
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
            let expected = kind_label(Some(kind)).as_str();
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
