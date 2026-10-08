// Pure mappings for ACP session updates and tool calls. Every tool
// execution becomes one `▸` status line shaped as `{label}: {body}`.
use crate::acp::{ContentBlock, SessionUpdate, ToolCall, ToolCallStatus, ToolCallUpdate, ToolKind};
use crate::types::{ToolKindLabel, ToolLineView, ToolStatus};

/// Skill titles arrive as `Loaded skill: X`; only `X` is shown.
const SKILL_PREFIX: &str = "Loaded skill: ";

/// Raw input keys shown when a call has no title, in priority order.
const KEY_PARAMS: &[&str] = &[
    "path",
    "file",
    "pattern",
    "query",
    "command",
    "url",
    "description",
    "skill",
    "name",
];

/// Extract streamed agent text from an update, if any. Pure.
pub fn agent_text_of(update: &SessionUpdate) -> Option<String> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => text_of_block(&chunk.content),
        _ => None,
    }
}

/// Extract streamed thought text from an update, if any. Pure.
pub fn thought_text_of(update: &SessionUpdate) -> Option<String> {
    match update {
        SessionUpdate::AgentThoughtChunk(chunk) => text_of_block(&chunk.content),
        _ => None,
    }
}

/// Extract user message text from an update, if any. Pure.
pub fn user_text_of(update: &SessionUpdate) -> Option<String> {
    match update {
        SessionUpdate::UserMessageChunk(chunk) => text_of_block(&chunk.content),
        _ => None,
    }
}

fn text_of_block(block: &ContentBlock) -> Option<String> {
    match block {
        ContentBlock::Text(text) => Some(text.text.clone()),
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

/// Merge a partial update onto the stored call. Partial updates only carry
/// changed fields, so the label and body come from the merged call. A missing
/// entry synthesizes from the update's title, then merges the rest. Pure.
pub fn with_update(existing: Option<ToolCall>, update: &ToolCallUpdate) -> ToolCall {
    let mut call = existing.unwrap_or_else(|| {
        ToolCall::new(
            update.tool_call_id.clone(),
            update.fields.title.clone().unwrap_or_default(),
        )
    });
    call.update(update.fields.clone());
    call
}

/// Format one tool line. Rows stay single-line ellipsis via CSS; the hover
/// tooltip reads the same text. The agent title renders verbatim after the
/// label; ongoing statuses append ` …`. Pure.
pub fn format_tool_line(call: &ToolCall) -> ToolLineView {
    let status = to_tool_status(call.status);
    ToolLineView {
        id: call.tool_call_id.to_string(),
        text: line_text(
            &call.title,
            call.name.as_deref(),
            Some(call.kind),
            call.raw_input.as_ref(),
            status,
        ),
        status,
    }
}

fn line_text(
    title: &str,
    name: Option<&str>,
    kind: Option<ToolKind>,
    raw_input: Option<&serde_json::Value>,
    status: ToolStatus,
) -> String {
    let label = tool_label(name, kind);
    let body = tool_body(title, raw_input);
    let base = if body.is_empty() {
        label
    } else {
        format!("{label}: {body}")
    };
    with_ongoing_suffix(&base, status)
}

/// Wire name when present, else the kind label. Names render as-is.
pub fn tool_label(name: Option<&str>, kind: Option<ToolKind>) -> String {
    match name.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => kind_label(kind).as_str().to_string(),
    }
}

/// Fallback label when the wire name is absent. Pure.
fn kind_label(kind: Option<ToolKind>) -> ToolKindLabel {
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

/// Titled calls show the title, except skill titles show only the skill name
/// and fetch titles show the `url` param (which omits the title's mime
/// suffix). Untitled calls show the first present key param.
fn tool_body(title: &str, raw_input: Option<&serde_json::Value>) -> String {
    let title = title.trim();
    if !title.is_empty() {
        if let Some(skill) = title.strip_prefix(SKILL_PREFIX) {
            return skill.trim().to_string();
        }
        if let Some(url) = raw_input.and_then(|input| first_line_param(input, "url")) {
            return url;
        }
        return title.to_string();
    }
    fallback_body(raw_input).unwrap_or_default()
}

fn fallback_body(raw_input: Option<&serde_json::Value>) -> Option<String> {
    let input = raw_input?;
    KEY_PARAMS
        .iter()
        .find_map(|key| first_line_param(input, key))
}

/// First line of a string param, trimmed. Non-string and blank values are
/// absent so callers can tell "no usable value" apart from a value.
fn first_line_param(input: &serde_json::Value, key: &str) -> Option<String> {
    let first = input.get(key)?.as_str()?.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(first.to_string())
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
        assert_eq!(user_text_of(&update), None);
        assert_eq!(thought_text_of(&update), None);
    }

    #[test]
    fn user_chunk_extracts_text_only() {
        let update = SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("do things"),
        )));
        assert_eq!(user_text_of(&update), Some("do things".to_string()));
        assert_eq!(agent_text_of(&update), None);
        assert_eq!(thought_text_of(&update), None);
    }

    #[test]
    fn thought_chunk_extracts_text() {
        let update = SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("considering"),
        )));
        assert_eq!(thought_text_of(&update), Some("considering".to_string()));
        assert_eq!(agent_text_of(&update), None);
        assert_eq!(user_text_of(&update), None);
    }

    #[test]
    fn non_text_update_yields_no_chunk() {
        let call = ToolCall::new("id-1", "title").kind(ToolKind::Read);
        let update = SessionUpdate::ToolCall(call);
        assert_eq!(agent_text_of(&update), None);
        assert_eq!(thought_text_of(&update), None);
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
    fn tool_lines_read_as_label_colon_body() {
        for (wire, view, suffix) in [
            (ToolCallStatus::Completed, ToolStatus::Completed, ""),
            (ToolCallStatus::Failed, ToolStatus::Failed, ""),
            (ToolCallStatus::Pending, ToolStatus::Pending, " …"),
            (ToolCallStatus::InProgress, ToolStatus::InProgress, " …"),
        ] {
            let call = ToolCall::new("id-1", "src/types.ts")
                .name("read")
                .kind(ToolKind::Read)
                .status(wire);
            let line = format_tool_line(&call);
            assert_eq!(line.text, format!("read: src/types.ts{suffix}"), "{wire:?}");
            assert_eq!(line.status, view);
        }

        let kind_only = ToolCall::new("id-1", "edit file.md")
            .kind(ToolKind::Edit)
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&kind_only).text, "edit: edit file.md");

        let named = ToolCall::new("id-1", "Explore plans UI")
            .name("task")
            .kind(ToolKind::Other)
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&named).text, "task: Explore plans UI");
    }

    #[test]
    fn skill_and_fetch_bodies_normalize() {
        let skill = ToolCall::new("id-1", "Loaded skill: repo-news")
            .name("skill")
            .kind(ToolKind::Other)
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&skill).text, "skill: repo-news");

        let fetch = ToolCall::new("id-1", "https://opencode.ai/docs (text/html)")
            .kind(ToolKind::Fetch)
            .raw_input(serde_json::json!({"url": "https://opencode.ai/docs"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(
            format_tool_line(&fetch).text,
            "fetch: https://opencode.ai/docs"
        );
    }

    #[test]
    fn untitled_calls_show_key_param() {
        let glob = ToolCall::new("id-1", "")
            .name("glob")
            .kind(ToolKind::Other)
            .raw_input(serde_json::json!({"pattern": "src/components/*.tsx"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&glob).text, "glob: src/components/*.tsx");

        let failed = ToolCall::new("id-1", "  ")
            .name("write")
            .kind(ToolKind::Edit)
            .raw_input(serde_json::json!({"path": "/var/tool-demo.txt"}))
            .status(ToolCallStatus::Failed);
        assert_eq!(format_tool_line(&failed).text, "write: /var/tool-demo.txt");

        let ongoing = ToolCall::new("id-1", "")
            .name("bash")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "git status"}))
            .status(ToolCallStatus::Pending);
        assert_eq!(format_tool_line(&ongoing).text, "bash: git status …");

        let multiline = ToolCall::new("id-1", "")
            .name("bash")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "first\nsecond"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&multiline).text, "bash: first");
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
    fn status_only_update_keeps_label_and_drops_suffix() {
        let existing = ToolCall::new("id-9", "")
            .name("bash")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "git status"}))
            .status(ToolCallStatus::InProgress);
        assert_eq!(format_tool_line(&existing).text, "bash: git status …");
        let update = ToolCallUpdate::new(
            "id-9",
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        );
        let merged = with_update(Some(existing), &update);
        assert_eq!(format_tool_line(&merged).text, "bash: git status");
    }

    #[test]
    fn body_and_kind_deltas_merge() {
        let existing = ToolCall::new("id-1", "old")
            .name("read")
            .kind(ToolKind::Read)
            .status(ToolCallStatus::InProgress);
        let body_delta = ToolCallUpdate::new("id-1", ToolCallUpdateFields::new().title("new"));
        let merged = with_update(Some(existing), &body_delta);
        assert_eq!(format_tool_line(&merged).text, "read: new …");

        let existing = ToolCall::new("id-1", "file")
            .kind(ToolKind::Read)
            .status(ToolCallStatus::Completed);
        let kind_delta =
            ToolCallUpdate::new("id-1", ToolCallUpdateFields::new().kind(ToolKind::Edit));
        let merged = with_update(Some(existing), &kind_delta);
        assert_eq!(format_tool_line(&merged).text, "edit: file");
    }

    #[test]
    fn missing_entry_synthesizes_then_merges() {
        let update = ToolCallUpdate::new(
            "id-9",
            ToolCallUpdateFields::new()
                .title("edit file.md")
                .name("edit")
                .status(ToolCallStatus::Completed),
        );
        let merged = with_update(None, &update);
        assert_eq!(format_tool_line(&merged).text, "edit: edit file.md");

        let bare = ToolCallUpdate::new(
            "id-9",
            ToolCallUpdateFields::new()
                .kind(ToolKind::Edit)
                .status(ToolCallStatus::Completed),
        );
        assert_eq!(format_tool_line(&with_update(None, &bare)).text, "edit");
    }

    #[test]
    fn merged_skill_and_fetch_bodies_normalize() {
        let skill = ToolCall::new("id-1", "Loaded skill: repo-news")
            .name("skill")
            .kind(ToolKind::Other)
            .status(ToolCallStatus::InProgress);
        let done = ToolCallUpdate::new(
            "id-1",
            ToolCallUpdateFields::new().status(ToolCallStatus::Completed),
        );
        assert_eq!(
            format_tool_line(&with_update(Some(skill), &done)).text,
            "skill: repo-news"
        );

        let fetch = ToolCall::new("id-1", "https://opencode.ai/docs (text/html)")
            .kind(ToolKind::Fetch)
            .raw_input(serde_json::json!({"url": "https://opencode.ai/docs"}))
            .status(ToolCallStatus::InProgress);
        assert_eq!(
            format_tool_line(&with_update(Some(fetch), &done)).text,
            "fetch: https://opencode.ai/docs"
        );
    }

    #[test]
    fn bare_label_when_nothing_to_show() {
        let mut call = ToolCall::new("id-1", "  ").kind(ToolKind::Execute);
        call.name = Some("edit".to_string());
        call.status = ToolCallStatus::Completed;
        assert_eq!(format_tool_line(&call).text, "edit");

        call.status = ToolCallStatus::Pending;
        assert_eq!(format_tool_line(&call).text, "edit …");

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
            assert_eq!(format_tool_line(&call).text, expected, "{kind:?}");
        }
    }
}
