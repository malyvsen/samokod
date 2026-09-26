import type {
	AppEvent,
	ConfigOptionView,
	PlansUpdate,
	SessionKey,
	SpendView,
	TodoView,
	TranscriptItem,
} from "../types";
import { sessionKeyOf } from "../types";

export interface ChatState {
	transcript: TranscriptItem[];
	todos: TodoView[];
	spend: SpendView | null;
	configOptions: ConfigOptionView[];
	working: boolean;
	approval: boolean;
	failed: boolean;
}

export type Chats = Record<string, ChatState>;

export function emptyChat(): ChatState {
	return {
		transcript: [],
		todos: [],
		spend: null,
		configOptions: [],
		working: false,
		approval: false,
		failed: false,
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

export function errorItem(
	raw: string,
	hint: string,
	retryable: boolean,
): TranscriptItem {
	return { kind: "error", id: crypto.randomUUID(), raw, hint, retryable };
}

/// Pure per-session event reducer. Session-keyed events route into their own
/// entry (background sessions update silently); global events leave the map
/// untouched so the caller handles them separately.
export function applySessionEvent(chats: Chats, event: AppEvent): Chats {
	switch (event.type) {
		case "plans_changed":
		case "branch_changed":
			return chats;
		case "agent_text": {
			const key = event.session;
			const chunk = event.chunk;
			return updateEntry(chats, key, (chat) => {
				const last = chat.transcript[chat.transcript.length - 1];
				const transcript: TranscriptItem[] =
					last !== undefined && last.kind === "agent"
						? [
								...chat.transcript.slice(0, -1),
								{ ...last, text: last.text + chunk },
							]
						: [
								...chat.transcript,
								{
									kind: "agent",
									id: crypto.randomUUID(),
									text: chunk,
								},
							];
				return { ...chat, working: true, failed: false, transcript };
			});
		}
		case "tool_line": {
			const key = event.session;
			const line = event.line;
			return updateEntry(chats, key, (chat) => {
				const index = chat.transcript.findIndex(
					(item) => item.kind === "tool" && item.line.id === line.id,
				);
				if (index >= 0) {
					const copy = [...chat.transcript];
					copy[index] = {
						kind: "tool",
						id: copy[index]?.id ?? crypto.randomUUID(),
						line,
					};
					return {
						...chat,
						working: true,
						failed: false,
						transcript: copy,
					};
				}
				return {
					...chat,
					working: true,
					failed: false,
					transcript: [
						...chat.transcript,
						{ kind: "tool", id: crypto.randomUUID(), line },
					],
				};
			});
		}
		case "turn_done":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				working: false,
				approval: false,
				failed: false,
			}));
		case "turn_failed":
		case "agent_exited":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				working: false,
				approval: false,
				failed: true,
				transcript: [
					...chat.transcript,
					errorItem(event.raw, event.hint, event.retryable),
				],
			}));
		case "permission_asked":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				approval: true,
				transcript: [
					...chat.transcript,
					{
						kind: "approval",
						id: crypto.randomUUID(),
						permission: event.permission,
						resolved: false,
					},
				],
			}));
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
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				todos: event.todos,
				transcript:
					event.changes.length > 0
						? [
								...chat.transcript,
								{
									kind: "todos",
									id: crypto.randomUUID(),
									changes: event.changes,
								},
							]
						: chat.transcript,
			}));
		case "spend_tick":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				spend: { cost: event.cost, contextPct: event.ctx_pct },
			}));
		case "session_reset":
			return updateEntry(chats, event.session, (chat) => ({
				...chat,
				todos: [],
				spend: null,
			}));
	}
}

/// Move the acted-on transcript to its history row when the plan renamed,
/// and open the new execution session working. `executorRunning` sets the
/// executor running for the eager execute path.
export function carryHistory(
	chats: Chats,
	key: SessionKey,
	update: PlansUpdate,
	executorRunning: boolean,
): Chats {
	const historyKey: SessionKey = {
		plan: update.selected.plan,
		role: key.role,
	};
	const next = { ...chats };
	const entry = next[sessionKeyOf(key)] ?? emptyChat();
	delete next[sessionKeyOf(key)];
	next[sessionKeyOf(historyKey)] = { ...entry, working: false };
	if (
		executorRunning &&
		sessionKeyOf(update.selected) !== sessionKeyOf(historyKey)
	) {
		const id = sessionKeyOf(update.selected);
		next[id] = {
			...(next[id] ?? emptyChat()),
			working: true,
			failed: false,
		};
	}
	return next;
}
