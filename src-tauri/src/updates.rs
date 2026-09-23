// Pure mapping from ACP session updates to transcript rows. Every tool
// execution becomes one `▸` status line regardless of kind.
use crate::acp::{ContentBlock, SessionUpdate, ToolCall, ToolCallUpdate, ToolKind};
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

/// Format one tool line. Rows stay single-line ellipsis via CSS; the hover
/// tooltip reads the same text. Pure.
pub fn format_tool_line(call: &ToolCall) -> ToolLineView {
    ToolLineView {
        id: call.tool_call_id.to_string(),
        text: tool_text(call),
        status: format!("{:?}", call.status).to_lowercase(),
    }
}

/// Format a tool update line by merging the update title when present. Pure.
pub fn format_tool_update(update: &ToolCallUpdate) -> ToolLineView {
    let title = update
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "running tool".to_string());
    ToolLineView {
        id: update.tool_call_id.to_string(),
        text: format!("{} {}", tool_verb(update.fields.kind), title),
        status: update
            .fields
            .status
            .map(|status| format!("{status:?}").to_lowercase())
            .unwrap_or_else(|| "pending".to_string()),
    }
}

fn tool_text(call: &ToolCall) -> String {
    let kind = tool_verb(Some(call.kind));
    if call.title.is_empty() {
        return kind.to_string();
    }
    format!("{kind} {}", call.title)
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

fn tool_verb(kind: Option<ToolKind>) -> &'static str {
    match kind {
        Some(ToolKind::Read) => "read",
        Some(ToolKind::Edit) => "edited",
        Some(ToolKind::Delete) => "deleted",
        Some(ToolKind::Move) => "moved",
        Some(ToolKind::Search) => "searched",
        Some(ToolKind::Execute) => "ran",
        Some(ToolKind::Think) => "thought",
        Some(ToolKind::Fetch) => "fetched",
        Some(ToolKind::SwitchMode) => "switched mode",
        _ => "ran",
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
    fn every_tool_kind_maps_to_one_line() {
        for kind in [
            ToolKind::Read,
            ToolKind::Edit,
            ToolKind::Delete,
            ToolKind::Move,
            ToolKind::Search,
            ToolKind::Execute,
            ToolKind::Think,
            ToolKind::Fetch,
            ToolKind::Other,
        ] {
            let call = ToolCall::new("id-1", "src-tauri/acp.rs")
                .kind(kind)
                .status(ToolCallStatus::Completed);
            let line = format_tool_line(&call);
            assert!(line.text.contains("src-tauri/acp.rs"), "{kind:?}");
            assert!(!line.id.is_empty());
        }
    }

    #[test]
    fn update_line_uses_title() {
        let update = ToolCallUpdate::new("id-9", ToolCallUpdateFields::default());
        let line = format_tool_update(&update);
        assert_eq!(line.id, "id-9");
    }
}
