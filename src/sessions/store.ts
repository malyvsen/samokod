import { assertNever } from "../assert";
import type {
	AppEvent,
	ConfigOptionView,
	PlansUpdate,
	SessionKey,
	TodoView,
} from "../bindings";
import type { TranscriptItem } from "../types";
import { sessionKeyOf } from "../types";
import { accumulateThought, freezeThought, type LiveThought } from "./thoughts";

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
	workingSince: number | null;
	approval: boolean;
	failed: boolean;
	start: SessionStart;
	live: LiveThought | null;
}

export type LiveTurn = {
	since: number;
	live: LiveThought | null;
};

export type Chats = Record<string, ChatState>;

export function emptyChat(): ChatState {
	return {
		transcript: [],
		todos: [],
		configOptions: [],
		working: false,
		workingSince: null,
		approval: false,
		failed: false,
		start: { kind: "idle" },
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

/// Optimistic send: working from send time, unacked until the backend
/// responds (command resolving or the first live event).
export function markSent(chat: ChatState): ChatState {
	return { ...chat, working: true, failed: false, workingSince: null };
}

/// Backend acknowledgment for a turn: presence of the timestamp is the ack.
/// Preserves an existing stamp so mid-turn answers never rebase the start.
export function markAcked(chat: ChatState, now: number): ChatState {
	return {
		...chat,
		working: true,
		failed: false,
		workingSince: chat.workingSince ?? now,
	};
}

/// Turn over (done, failed, stopped, or failed to send): no longer working
/// and no ack stamp. Callers set approval/failed alongside as needed.
export function markIdle(chat: ChatState): ChatState {
	return { ...chat, working: false, workingSince: null };
}

/// The acked turn driving the Working box, if any: working plus a stamp.
/// An optimistic `working` with no stamp is sent but not yet acknowledged.
export function liveTurnOf(
	chat: ChatState | null | undefined,
): LiveTurn | null {
	if (chat === undefined || chat === null) return null;
	if (!chat.working || chat.workingSince === null) return null;
	return { since: chat.workingSince, live: chat.live };
}

export function removeEntry(chats: Chats, key: SessionKey): Chats {
	const next = { ...chats };
	delete next[sessionKeyOf(key)];
	return next;
}

/// Arrival time for items created by live events. Replayed history stays
/// timestamp-less so runs without wall-clock read as timeless.
function arrival(replaying: boolean, now: number): { at?: number } {
	return replaying ? {} : { at: now };
}

/// The transcript with any pending live thought frozen ahead of the next
/// content event.
function frozen(chat: ChatState, now: number): TranscriptItem[] {
	return freezeThought(chat.transcript, chat.live, now);
}

export function errorItem(
	raw: string,
	hint: string,
	retryable: boolean,
	at?: number,
): TranscriptItem {
	return {
		kind: "error",
		id: crypto.randomUUID(),
		raw,
		hint,
		retryable,
		...(at === undefined ? {} : { at }),
	};
}

function appendText(
	transcript: TranscriptItem[],
	chunk: string,
	at?: number,
): TranscriptItem[] {
	const last = transcript[transcript.length - 1];
	if (last !== undefined && last.kind === "agent") {
		return [...transcript.slice(0, -1), { ...last, text: last.text + chunk }];
	}
	return [
		...transcript,
		{
			kind: "agent",
			id: crypto.randomUUID(),
			text: chunk,
			...(at === undefined ? {} : { at }),
		},
	];
}

/// Live transcript content with its ack: replayed history streams
/// timestamp-less without marking a turn, so the status dot stays truthful.
function ackWithTranscript(
	chat: ChatState,
	transcript: TranscriptItem[],
	now: number,
): ChatState {
	if (chat.start.kind === "replaying") {
		return { ...chat, transcript };
	}
	return { ...markAcked(chat, now), transcript };
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
		case "open_repos_changed":
			return chats;
		case "agent_text": {
			const key = event.session;
			const chunk = event.chunk;
			return updateEntry(chats, key, (chat) => {
				const replaying = chat.start.kind === "replaying";
				const transcript = appendText(
					frozen(chat, now),
					chunk,
					replaying ? undefined : now,
				);
				if (replaying) return { ...chat, transcript };
				return {
					...ackWithTranscript(chat, transcript, now),
					live: null,
				};
			});
		}
		case "agent_thought": {
			const key = event.session;
			const chunk = event.chunk;
			return updateEntry(chats, key, (chat) => {
				if (chat.start.kind === "replaying") return chat;
				return {
					...markAcked(chat, now),
					live: accumulateThought(chat.live, now, chunk),
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
					{
						kind: "user",
						id: crypto.randomUUID(),
						text: chunk,
						...arrival(chat.start.kind === "replaying", now),
					},
				],
			}));
		}
		case "tool_line": {
			const key = event.session;
			const line = event.line;
			return updateEntry(chats, key, (chat) => {
				const replaying = chat.start.kind === "replaying";
				const settled = frozen(chat, now);
				const index = settled.findIndex(
					(item) => item.kind === "tool" && item.line.id === line.id,
				);
				const transcript: TranscriptItem[] =
					index >= 0
						? settled.map((item, current) =>
								current === index && item.kind === "tool"
									? { ...item, line }
									: item,
							)
						: [
								...settled,
								{
									kind: "tool",
									id: crypto.randomUUID(),
									line,
									...arrival(replaying, now),
								},
							];
				if (replaying) return { ...chat, transcript };
				return {
					...ackWithTranscript(chat, transcript, now),
					live: null,
				};
			});
		}
		case "turn_done":
			return updateEntry(chats, event.session, (chat) => ({
				...markIdle(chat),
				approval: false,
				failed: false,
				transcript: frozen(chat, now),
				live: null,
			}));
		case "turn_failed":
		case "agent_exited":
			return updateEntry(chats, event.session, (chat) => ({
				...markIdle(chat),
				approval: false,
				failed: true,
				transcript: [
					...frozen(chat, now),
					errorItem(
						event.raw,
						event.hint,
						event.retryable,
						chat.start.kind === "replaying" ? undefined : now,
					),
				],
				live: null,
			}));
		case "permission_asked":
			return updateEntry(chats, event.session, (chat) => {
				// Replayed approvals arrive already answered, so they render
				// resolved without pausing for input.
				const replaying = chat.start.kind === "replaying";
				const transcript: TranscriptItem[] = [
					...frozen(chat, now),
					{
						kind: "approval",
						id: crypto.randomUUID(),
						permission: event.permission,
						resolved: replaying,
						...arrival(replaying, now),
					},
				];
				if (replaying) {
					return {
						...chat,
						approval: chat.approval,
						transcript,
						live: chat.live,
					};
				}
				return {
					...ackWithTranscript(chat, transcript, now),
					approval: true,
					live: null,
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
					...frozen(chat, now),
					{
						kind: "todos",
						id: crypto.randomUUID(),
						changes: event.changes,
						...arrival(replaying, now),
					},
				];
				return {
					...ackWithTranscript(chat, transcript, now),
					todos: event.todos,
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
						transcript: frozen(chat, now),
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
/// `markSelectedRunning` opens the new selected session working; callers
/// pass whether the acted plan is viewed so background work moves its own
/// bucket without touching the viewed transcript.
export function carryHistory(
	chats: Chats,
	key: SessionKey,
	update: PlansUpdate,
	markSelectedRunning: boolean,
	now: number,
): Chats {
	const historyKey: SessionKey = {
		plan: update.selected.plan,
		role: key.role,
	};
	const next = { ...chats };
	const entry = next[sessionKeyOf(key)] ?? emptyChat();
	delete next[sessionKeyOf(key)];
	next[sessionKeyOf(historyKey)] = {
		...markIdle(entry),
		live: null,
	};
	if (
		markSelectedRunning &&
		sessionKeyOf(update.selected) !== sessionKeyOf(historyKey)
	) {
		const id = sessionKeyOf(update.selected);
		next[id] = markAcked({ ...(next[id] ?? emptyChat()), live: null }, now);
	}
	return next;
}
