// Pure mappings for ACP session updates and tool calls. Every tool
// execution becomes one `▸` status line shaped as `{label}: {body}`.
// Rendered updates (agent text and thought, user replay, tool call and
// update, usage, config options) feed the transcript; every other
// `SessionUpdate` variant routes to one explicit ignore with a structural
// debug log so protocol drift stays visible. Rendering never changes here.
use crate::acp::{
    ContentBlock, Meta, SessionNotification, SessionUpdate, ToolCall, ToolCallStatus,
    ToolCallUpdate, ToolKind,
};
use crate::types::{ToolKindLabel, ToolLineView, ToolStatus};

/// Skill titles arrive as `Loaded skill: X`; only `X` is shown.
const SKILL_PREFIX: &str = "Loaded skill: ";

/// Raw input keys shown when a call has no usable title, in priority order.
/// `pattern` leads so search lines match grep's completion title;
/// `filePath` covers OpenCode reads.
const KEY_PARAMS: &[&str] = &[
    "pattern",
    "path",
    "file",
    "filePath",
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
/// this client never enumerated: future drift, not a known ignore. Pure.
pub fn update_kind(update: &SessionUpdate) -> Option<&'static str> {
    match update {
        SessionUpdate::UserMessageChunk(_) => Some("user_message_chunk"),
        SessionUpdate::AgentMessageChunk(_) => Some("agent_message_chunk"),
        SessionUpdate::AgentThoughtChunk(_) => Some("agent_thought_chunk"),
        SessionUpdate::ToolCall(_) => Some("tool_call"),
        SessionUpdate::ToolCallUpdate(_) => Some("tool_call_update"),
        SessionUpdate::Plan(_) => Some("plan"),
        SessionUpdate::PlanUpdate(_) => Some("plan_update"),
        SessionUpdate::PlanRemoved(_) => Some("plan_removed"),
        SessionUpdate::AvailableCommandsUpdate(_) => Some("available_commands"),
        SessionUpdate::CurrentModeUpdate(_) => Some("current_mode"),
        SessionUpdate::ConfigOptionUpdate(_) => Some("config_option"),
        SessionUpdate::SessionInfoUpdate(_) => Some("session_info"),
        SessionUpdate::UsageUpdate(_) => Some("usage"),
        SessionUpdate::Notice(_) => Some("notice"),
        SessionUpdate::CompactionUpdate(_) => Some("compaction"),
        SessionUpdate::CompactionSummaryChunk(_) => Some("compaction_summary_chunk"),
        SessionUpdate::SubagentUpdate(_) => Some("subagent"),
        SessionUpdate::SessionMessage(_) => Some("session_message"),
        SessionUpdate::SessionMessageChunk(_) => Some("session_message_chunk"),
        _ => None,
    }
}

/// True when the update feeds the transcript (streamed text and thought,
/// user replay, tool call and update, usage, config options). Everything
/// else is an explicit ignore. Implemented as the negation of
/// `ignored_summary` so the rendered set lives in one match. Pure.
pub fn is_rendered(update: &SessionUpdate) -> bool {
    ignored_summary(update).is_none()
}

/// Structural one-line summary for an ignored update: counts, ids, and
/// present meta keys, never full payloads. `None` means the update feeds
/// the transcript; `Some("unknown")` means future protocol drift. Pure.
pub fn ignored_summary(update: &SessionUpdate) -> Option<String> {
    let summary = match update {
        SessionUpdate::UserMessageChunk(_)
        | SessionUpdate::AgentMessageChunk(_)
        | SessionUpdate::AgentThoughtChunk(_)
        | SessionUpdate::ToolCall(_)
        | SessionUpdate::ToolCallUpdate(_)
        | SessionUpdate::UsageUpdate(_)
        | SessionUpdate::ConfigOptionUpdate(_) => return None,
        SessionUpdate::Plan(plan) => format!("plan entries={}", plan.entries.len()),
        SessionUpdate::PlanUpdate(_) => "plan_update replace".to_string(),
        SessionUpdate::PlanRemoved(_) => "plan_removed".to_string(),
        SessionUpdate::AvailableCommandsUpdate(update) => {
            format!(
                "available_commands count={}",
                update.available_commands.len()
            )
        }
        SessionUpdate::CurrentModeUpdate(update) => {
            format!("current_mode id={}", update.current_mode_id)
        }
        SessionUpdate::SessionInfoUpdate(update) => {
            let title = if update.title.is_undefined() {
                "absent"
            } else if update.title.is_null() {
                "cleared"
            } else {
                "set"
            };
            format!(
                "session_info title={title} meta_keys=[{}]",
                meta_keys(update.meta.as_ref()).join(",")
            )
        }
        SessionUpdate::Notice(notice) => {
            format!(
                "notice severity={:?} title_len={}",
                notice.severity,
                notice.title.len()
            )
        }
        SessionUpdate::CompactionUpdate(update) => format!(
            "compaction id={} status={:?}",
            update.compaction_id, update.status
        ),
        SessionUpdate::CompactionSummaryChunk(update) => {
            format!("compaction_summary_chunk id={}", update.compaction_id)
        }
        SessionUpdate::SubagentUpdate(update) => format!(
            "subagent session={} meta_keys=[{}]",
            update.session_id,
            meta_keys(update.meta.value()).join(",")
        ),
        SessionUpdate::SessionMessage(message) => {
            format!("session_message id={}", message.message_id)
        }
        SessionUpdate::SessionMessageChunk(chunk) => {
            format!("session_message_chunk id={}", chunk.message_id)
        }
        _ => "unknown".to_string(),
    };
    Some(summary)
}

/// One debug line for an ignored notification: kind plus structural
/// summary plus `_meta` keys and OpenCode retry/compaction markers and
/// child association. Rendered updates report `rendered:<kind>` so the
/// same call logs uniformly. Pure.
pub fn ignore_detail(notification: &SessionNotification) -> String {
    let kind = update_kind(&notification.update).unwrap_or("unknown");
    let summary =
        ignored_summary(&notification.update).unwrap_or_else(|| format!("rendered:{kind}"));
    format!(
        "{kind} {summary} {}",
        notification_meta_summary(notification)
    )
}

/// OpenCode extension keys carried in `_meta`, logged structurally so
/// vendor drift stays visible without changing rendering.
pub const RETRY_META_KEY: &str = "opencode/retry";
pub const COMPACTION_META_KEY: &str = "opencode/compaction";

/// Sorted `_meta` key list for structural logging. Pure.
fn meta_keys(meta: Option<&Meta>) -> Vec<String> {
    let mut keys: Vec<String> = meta
        .map(|meta| meta.keys().cloned().collect())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// Short string for one OpenCode `_meta` marker, when present. Pure.
fn marker_of(meta: Option<&Meta>, key: &str) -> Option<String> {
    meta?.get(key).map(|value| value.to_string())
}

/// Child session id announced by a subagent update, when present. Pure.
fn subagent_session_of(update: &SessionUpdate) -> Option<String> {
    match update {
        SessionUpdate::SubagentUpdate(update) => Some(update.session_id.to_string()),
        _ => None,
    }
}

/// Notification-level `_meta` summary: key list plus the OpenCode retry
/// and compaction markers and any child/subagent association, when
/// present. Empty string means no meta at all. Pure.
pub fn notification_meta_summary(notification: &SessionNotification) -> String {
    let meta = notification.meta.as_ref();
    let mut parts = vec![format!("keys=[{}]", meta_keys(meta).join(","))];
    if let Some(retry) = marker_of(meta, RETRY_META_KEY) {
        parts.push(format!("retry={retry}"));
    }
    if let Some(compaction) = marker_of(meta, COMPACTION_META_KEY) {
        parts.push(format!("compaction={compaction}"));
    }
    if let Some(child) = subagent_session_of(&notification.update) {
        parts.push(format!("child={child}"));
    }
    parts.join(" ")
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
    let label = tool_label(call.name.as_deref(), Some(call.kind));
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
        label,
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
    let body = tool_body(title, kind, raw_input);
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
        _ => ToolKindLabel::Other,
    }
}

/// Titled calls show the title, except skill titles show only the skill name
/// and fetch titles show the `url` param (which omits the title's mime
/// suffix). Missing and placeholder titles show the first present key param.
fn tool_body(title: &str, kind: Option<ToolKind>, raw_input: Option<&serde_json::Value>) -> String {
    let title = title.trim();
    if let Some(skill) = title.strip_prefix(SKILL_PREFIX) {
        return skill.trim().to_string();
    }
    if !title.is_empty() {
        if let Some(url) = raw_input.and_then(|input| first_line_param(input, "url")) {
            return url;
        }
        if !is_placeholder_title(kind, title) {
            return title.to_string();
        }
    }
    fallback_body(raw_input).unwrap_or_else(|| title.to_string())
}

/// Placeholder titles carry no summary, so the input summary replaces them.
/// Both V1 (`bash`) and V2 (`shell`) shell titles count. Pure.
fn is_placeholder_title(kind: Option<ToolKind>, title: &str) -> bool {
    matches!(
        (kind, title),
        (Some(ToolKind::Read), "read")
            | (Some(ToolKind::Edit), "edit" | "write")
            | (Some(ToolKind::Delete), "delete")
            | (Some(ToolKind::Move), "move")
            | (Some(ToolKind::Search), "glob" | "grep")
            | (Some(ToolKind::Execute), "bash" | "shell")
            | (Some(ToolKind::Think), "think")
            | (Some(ToolKind::Fetch), "fetch")
            | (Some(ToolKind::SwitchMode), "mode")
    )
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
    use crate::acp::{
        ContentBlock, SessionUpdate, TextContent, ToolCall, ToolCallStatus, ToolCallUpdate,
        ToolKind,
    };
    use agent_client_protocol::v1::schema::{
        AvailableCommand, AvailableCommandsUpdate, CompactionStatus, CompactionSummaryChunk,
        CompactionUpdate, ConfigOptionUpdate, ContentChunk, CurrentModeUpdate, Notice,
        NoticeSeverity, Plan, PlanRemoved, PlanUpdate, PlanUpdateContent, SessionInfoUpdate,
        SessionMessage, SessionMessageChunk, SubagentUpdate, ToolCallUpdateFields, UsageUpdate,
    };

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
    fn calls_without_summary_show_key_param() {
        let glob = ToolCall::new("id-1", "")
            .name("glob")
            .kind(ToolKind::Other)
            .raw_input(serde_json::json!({"pattern": "src/components/*.tsx"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&glob).text, "glob: src/components/*.tsx");

        let placeholder_glob = ToolCall::new("id-1", "glob")
            .kind(ToolKind::Search)
            .raw_input(serde_json::json!({"pattern": "src/**/*.ts"}))
            .status(ToolCallStatus::InProgress);
        assert_eq!(
            format_tool_line(&placeholder_glob).text,
            "search: src/**/*.ts …"
        );

        let placeholder_grep = ToolCall::new("id-1", "grep")
            .kind(ToolKind::Search)
            .raw_input(serde_json::json!({"pattern": "ToolLineView", "path": "src"}))
            .status(ToolCallStatus::InProgress);
        assert_eq!(
            format_tool_line(&placeholder_grep).text,
            "search: ToolLineView …"
        );

        let placeholder_read = ToolCall::new("id-1", "read")
            .kind(ToolKind::Read)
            .raw_input(serde_json::json!({"filePath": "/tmp/src/types.ts"}))
            .status(ToolCallStatus::InProgress);
        assert_eq!(
            format_tool_line(&placeholder_read).text,
            "read: /tmp/src/types.ts …"
        );

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

        let placeholder_bash = ToolCall::new("id-1", "bash")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "ls -la"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&placeholder_bash).text, "bash: ls -la");

        let placeholder_shell = ToolCall::new("id-1", "shell")
            .name("shell")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "ls -la"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&placeholder_shell).text, "shell: ls -la");

        let multiline = ToolCall::new("id-1", "")
            .name("bash")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "first\nsecond"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&multiline).text, "bash: first");

        let placeholder_without_input = ToolCall::new("id-1", "read")
            .kind(ToolKind::Read)
            .status(ToolCallStatus::Completed);
        assert_eq!(
            format_tool_line(&placeholder_without_input).text,
            "read: read"
        );
    }

    #[test]
    fn titled_calls_keep_title_over_input() {
        let read = ToolCall::new("id-1", "src/types.ts")
            .kind(ToolKind::Read)
            .raw_input(serde_json::json!({"filePath": "/tmp/src/types.ts"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&read).text, "read: src/types.ts");

        let grep = ToolCall::new("id-1", "ToolLineView")
            .kind(ToolKind::Search)
            .raw_input(serde_json::json!({"pattern": "ToolLineView", "path": "src"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&grep).text, "search: ToolLineView");

        let bash = ToolCall::new("id-1", "ls -la")
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({"command": "ls -la"}))
            .status(ToolCallStatus::Completed);
        assert_eq!(format_tool_line(&bash).text, "bash: ls -la");
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
    fn label_only_when_nothing_to_show() {
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

    #[test]
    fn every_variant_has_a_kind() {
        let updates = vec![
            SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("do things"),
            ))),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("hello"),
            ))),
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("considering"),
            ))),
            SessionUpdate::ToolCall(ToolCall::new("id-1", "title")),
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new("id-1", ToolCallUpdateFields::new())),
            SessionUpdate::Plan(Plan::new(vec![])),
            SessionUpdate::PlanUpdate(PlanUpdate::new(PlanUpdateContent::markdown(
                "plan-1", "# plan",
            ))),
            SessionUpdate::PlanRemoved(PlanRemoved::new("plan-1")),
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![
                AvailableCommand::new("research", "research the codebase"),
            ])),
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("build")),
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(vec![])),
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new()),
            SessionUpdate::UsageUpdate(UsageUpdate::new(1, 2)),
            SessionUpdate::Notice(Notice::new(NoticeSeverity::Info, "heads up")),
            SessionUpdate::CompactionUpdate(CompactionUpdate::new(
                "compaction-1",
                CompactionStatus::Completed,
            )),
            SessionUpdate::CompactionSummaryChunk(CompactionSummaryChunk::new(
                "compaction-1",
                ContentBlock::Text(TextContent::new("summary")),
            )),
            SessionUpdate::SubagentUpdate(SubagentUpdate::new("ses_child")),
            SessionUpdate::SessionMessage(SessionMessage::new("msg-1")),
            SessionUpdate::SessionMessageChunk(SessionMessageChunk::new(
                "msg-1",
                ContentBlock::Text(TextContent::new("chunk")),
            )),
        ];
        let kinds: Vec<Option<&str>> = updates.iter().map(update_kind).collect();
        assert!(
            kinds.iter().all(|kind| kind.is_some()),
            "every backend variant enumerates, got {kinds:?}"
        );
        assert_eq!(update_kind(&updates[0]), Some("user_message_chunk"));
        assert_eq!(update_kind(&updates[5]), Some("plan"));
        assert_eq!(update_kind(&updates[8]), Some("available_commands"));
        assert_eq!(update_kind(&updates[9]), Some("current_mode"));
        assert_eq!(update_kind(&updates[11]), Some("session_info"));
        assert_eq!(update_kind(&updates[12]), Some("usage"));
        assert_eq!(update_kind(&updates[13]), Some("notice"));
        assert_eq!(update_kind(&updates[14]), Some("compaction"));
        assert_eq!(update_kind(&updates[16]), Some("subagent"));
    }

    #[test]
    fn only_transcript_updates_render() {
        for update in [
            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                TextContent::new("hello"),
            ))),
            SessionUpdate::ToolCall(ToolCall::new("id-1", "title")),
            SessionUpdate::UsageUpdate(UsageUpdate::new(1, 2)),
            SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(vec![])),
        ] {
            assert!(is_rendered(&update), "{update:?} renders");
        }
        for update in [
            SessionUpdate::Plan(Plan::new(vec![])),
            SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(vec![])),
            SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("build")),
            SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new()),
            SessionUpdate::Notice(Notice::new(NoticeSeverity::Info, "heads up")),
            SessionUpdate::SubagentUpdate(SubagentUpdate::new("ses_child")),
        ] {
            assert!(!is_rendered(&update), "{update:?} stays ignored");
            assert_eq!(agent_text_of(&update), None);
            assert_eq!(thought_text_of(&update), None);
            assert_eq!(user_text_of(&update), None);
        }
    }

    #[test]
    fn ignored_summaries_stay_structural() {
        let summary = ignored_summary(&SessionUpdate::AvailableCommandsUpdate(
            AvailableCommandsUpdate::new(vec![AvailableCommand::new("research", "research")]),
        ))
        .expect("ignored");
        assert!(summary.contains("count=1"), "{summary}");
        let summary = ignored_summary(&SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(
            "build",
        )))
        .expect("ignored");
        assert!(summary.contains("build"), "{summary}");
        let summary = ignored_summary(&SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new()))
            .expect("ignored");
        assert!(summary.contains("session_info"), "{summary}");
        let summary = ignored_summary(&SessionUpdate::Plan(Plan::new(vec![]))).expect("ignored");
        assert!(summary.contains("entries=0"), "{summary}");
        let summary = ignored_summary(&SessionUpdate::Notice(Notice::new(
            NoticeSeverity::Warning,
            "heads up",
        )))
        .expect("ignored");
        assert!(summary.contains("notice"), "{summary}");
        let summary = ignored_summary(&SessionUpdate::CompactionUpdate(CompactionUpdate::new(
            "compaction-1",
            CompactionStatus::InProgress,
        )))
        .expect("ignored");
        assert!(summary.contains("compaction-1"), "{summary}");
        let summary = ignored_summary(&SessionUpdate::SubagentUpdate(SubagentUpdate::new(
            "ses_child",
        )))
        .expect("ignored");
        assert!(summary.contains("ses_child"), "{summary}");
        assert_eq!(
            ignored_summary(&SessionUpdate::AgentMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new("hello"))
            ))),
            None
        );
        let detail = ignore_detail(&SessionNotification::new(
            "ses_1",
            SessionUpdate::SubagentUpdate(SubagentUpdate::new("ses_child")),
        ));
        assert!(detail.contains("ses_child"), "{detail}");
        assert!(detail.contains("subagent"), "{detail}");
    }

    #[test]
    fn notification_meta_parses_retry_compaction_and_child() {
        let mut meta = Meta::new();
        meta.insert(
            RETRY_META_KEY.to_string(),
            serde_json::json!({"attempt": 2}),
        );
        meta.insert(
            COMPACTION_META_KEY.to_string(),
            serde_json::json!("completed"),
        );
        let notification =
            SessionNotification::new("ses_1", SessionUpdate::Plan(Plan::new(vec![]))).meta(meta);
        let summary = notification_meta_summary(&notification);
        assert!(summary.contains("opencode/retry"), "{summary}");
        assert!(summary.contains("opencode/compaction"), "{summary}");
        let detail = ignore_detail(&notification);
        assert!(detail.contains("opencode/retry"), "{detail}");
        assert!(detail.contains("opencode/compaction"), "{detail}");
        assert!(detail.contains("plan"), "{detail}");
        let bare = SessionNotification::new("ses_1", SessionUpdate::Plan(Plan::new(vec![])));
        assert_eq!(notification_meta_summary(&bare), "keys=[]");
        assert_eq!(
            ignore_detail(&bare),
            format!(
                "plan {} keys=[]",
                ignored_summary(&SessionUpdate::Plan(Plan::new(vec![]))).expect("ignored")
            )
        );
    }
}
