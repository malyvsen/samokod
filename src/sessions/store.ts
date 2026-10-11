import { assertNever } from "../assert";
import type {
	AppEvent,
	ConfigOptionView,
	PlansUpdate,
	SessionKey,
	TodoView,
	TranscriptItem,
} from "../types";
import { sessionKeyOf } from "../types";
import { freezeBurst, type LiveStatus, liveForThought } from "./thoughts";

export type SessionStart =
	| { kind: "idle" }
	| { kind: "preparing" }
	| { kind: "replaying" }
	| {
			kind: "failed";
			error: { raw: string; hint: string; retryable: boolean };
	  };

export interface ChatState {
	transcript: TranscriptItem[];
	todos: TodoView[];
	configOptions: ConfigOptionView[];
	working: boolean;
	approval: boolean;
	failed: boolean;
	start: SessionStart;
	scopingPreview: string | null;
	live: LiveStatus | null;
}

export type Chats = Record<string, ChatState>;

export function emptyChat(): ChatState {
	return {
		transcript: [],
		todos: [],
		configOptions: [],
		working: false,
		approval: false,
		failed: false,
		start: { kind: "idle" },
		scopingPreview: null,
		live: null,
	};
}

export function updateEntry(
	chats: Chats,
	key: SessionKey,
	next: (chat: ChatState) => ChatState,
): Chats {
	const id = sessionKeyOf(key);
	return { ...chats, [id]: next(chats[id] ?? emptyChat()) };
}

export function removeEntry(chats: Chats, key: SessionKey): Chats {
	const next = { ...chats };
	delete next[sessionKeyOf(key)];
	return next;
}

/// Preview from a response payload, set once. Null never sets and an
/// existing preview never overwrites, so the lead stays stable across
/// responses. Identical values keep the same object.
export function withScopingPreview(
	chats: Chats,
	key: SessionKey,
	preview: string | null,
): Chats {
	return updateEntry(chats, key, (chat) => {
		if (preview === null) return chat;
		if (chat.scopingPreview !== null) return chat;
		return { ...chat, scopingPreview: preview };
	});
}

export function errorItem(
	raw: string,
	hint: string,
	retryable: boolean,
): TranscriptItem {
	return { kind: "error", id: crypto.randomUUID(), raw, hint, retryable };
}

function appendText(
	transcript: TranscriptItem[],
	chunk: string,
): TranscriptItem[] {
	const last = transcript[transcript.length - 1];
	if (last !== undefined && last.kind === "agent") {
		return [...transcript.slice(0, -1), { ...last, text: last.text + chunk }];
	}
	return [
		...transcript,
		{ kind: "agent", id: crypto.randomUUID(), text: chunk },
	];
}

/// Replayed transcript streams while history replays without marking a live
/// turn, so the status dot stays truthful. Live updates mark working.
function withTranscript(
	chat: ChatState,
	transcript: TranscriptItem[],
): ChatState {
	if (chat.start.kind === "replaying") {
		return { ...chat, transcript };
	}
	return { ...chat, working: true, failed: false, transcript };
}

/// Enter a start phase. Retrying after a failure replays from empty.
function beginStart(
	chat: ChatState,
	start: Extract<SessionStart, { kind: "preparing" | "replaying" }>,
): ChatState {
	return {
		...chat,
		start,
		transcript: chat.start.kind === "failed" ? [] : chat.transcript,
	};
}

/// Pure per-session event reducer. Session-keyed events route into their own
/// entry (background sessions update silently); global events leave the map
/// untouched so the caller handles them separately.
export function applySessionEvent(
	chats: Chats,
	event: AppEvent,
	now: number = Date.now(),
): Chats {
	switch (event.type) {
		case "plans_changed":
		case "branch_changed":
			return chats;
		case "agent_text": {
			const key = event.session;
			const chunk = event.chunk;
			return updateEntry(chats, key, (chat) => {
				const frozen = freezeBurst(chat.transcript, chat.live);
				const transcript = appendText(frozen, chunk);
				if (chat.start.kind === "replaying") return { ...chat, transcript };
				return {
					...withTranscript(chat, transcript),
					live: null,
				};
			});
		}
		case "agent_thought": {
			const key = event.session;
			return updateEntry(chats, key, (chat) => {
				if (chat.start.kind === "replaying") return chat;
				return {
					...chat,
					working: true,
					failed: false,
					live: liveForThought(chat.live, now),
				};
			});
		}
		case "user_text": {
			const key = event.session;
			const chunk = event.chunk;
			// Each user message is its own bubble.
			return updateEntry(chats, key, (chat) => ({
				...chat,
				transcript: [
					...chat.transcript,
					{ kind: "user", id: crypto.randomUUID(), text: chunk },
				],
			}));
		}
		case "tool_line": {
			const key = event.session;
			const line = event.line;
			return updateEntry(chats, key, (chat) => {
				const frozen = freezeBurst(chat.transcript, chat.live);
				const index = frozen.findIndex(
					(item) => item.kind === "tool" && item.line.id === line.id,
				);
				const transcript: TranscriptItem[] =
					index >= 0
						? frozen.map((item, current) =>
								current === index && item.kind === "tool"
									? { ...item, line }
									: item,
							)
						: [...frozen, { kind: "tool", id: crypto.randomUUID(), line }];
				if (chat.start.kind === "replaying") return { ...chat, transcript };
				return {
					...withTranscript(chat, transcript),
					live: null,
				};
			});
		}
		case "turn_done":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				working: false,
				approval: false,
				failed: false,
				transcript: freezeBurst(chat.transcript, chat.live),
				live: null,
			}));
		case "turn_failed":
		case "agent_exited":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				working: false,
				approval: false,
				failed: true,
				transcript: [
					...freezeBurst(chat.transcript, chat.live),
					errorItem(event.raw, event.hint, event.retryable),
				],
				live: null,
			}));
		case "permission_asked":
			return updateEntry(chats, event.session, (chat) => {
				// Replayed approvals arrive already answered, so they render
				// resolved without pausing for input.
				const replaying = chat.start.kind === "replaying";
				const transcript: TranscriptItem[] = [
					...freezeBurst(chat.transcript, chat.live),
					{
						kind: "approval",
						id: crypto.randomUUID(),
						permission: event.permission,
						resolved: replaying,
					},
				];
				return {
					...chat,
					approval: replaying ? chat.approval : true,
					transcript,
					live: replaying ? chat.live : null,
				};
			});
		case "permission_resolved":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				transcript: chat.transcript.map((item) =>
					item.kind === "approval" &&
					item.permission.tool_call_id === event.tool_call_id
						? { ...item, resolved: true }
						: item,
				),
			}));
		case "config_options":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				configOptions: event.options,
			}));
		case "todos_changed":
			return updateEntry(chats, event.session, (chat) => {
				if (event.changes.length === 0) {
					return { ...chat, todos: event.todos };
				}
				const replaying = chat.start.kind === "replaying";
				const transcript: TranscriptItem[] = [
					...freezeBurst(chat.transcript, chat.live),
					{
						kind: "todos",
						id: crypto.randomUUID(),
						changes: event.changes,
					},
				];
				return {
					...chat,
					todos: event.todos,
					transcript,
					live: replaying ? chat.live : null,
				};
			});
		case "spend_tick":
			// Spend lives on the plans-list session status (the hover
			// tooltip); per-chat totals are obsolete.
			return chats;
		case "session_reset":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				todos: [],
			}));
		case "history_preparing":
			return updateEntry(chats, event.session, (chat) => ({
				...beginStart(chat, { kind: "preparing" }),
				live: null,
			}));
		case "history_begin":
			return updateEntry(chats, event.session, (chat) => ({
				...beginStart(
					{
						...chat,
						transcript: freezeBurst(chat.transcript, chat.live),
					},
					{ kind: "replaying" },
				),
				live: null,
			}));
		case "history_done":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				start: { kind: "idle" },
				live: null,
			}));
		case "history_failed":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				live: null,
				start: {
					kind: "failed",
					error: {
						raw: event.raw,
						hint: event.hint,
						retryable: event.retryable,
					},
				},
			}));
		default:
			return assertNever(event);
	}
}

/// Move the acted-on transcript to its history row when the plan renamed.
/// The carried preview always clears: a renamed plan is never fresh.
/// `markSelectedRunning` opens the new selected session working; callers
/// pass whether the acted plan is viewed so background work moves its own
/// bucket without touching the viewed transcript.
export function carryHistory(
	chats: Chats,
	key: SessionKey,
	update: PlansUpdate,
	markSelectedRunning: boolean,
): Chats {
	const historyKey: SessionKey = {
		plan: update.selected.plan,
		role: key.role,
	};
	const next = { ...chats };
	const entry = next[sessionKeyOf(key)] ?? emptyChat();
	delete next[sessionKeyOf(key)];
	next[sessionKeyOf(historyKey)] = {
		...entry,
		working: false,
		live: null,
		scopingPreview: null,
	};
	if (
		markSelectedRunning &&
		sessionKeyOf(update.selected) !== sessionKeyOf(historyKey)
	) {
		const id = sessionKeyOf(update.selected);
		next[id] = {
			...(next[id] ?? emptyChat()),
			working: true,
			failed: false,
			live: null,
		};
	}
	return next;
}
