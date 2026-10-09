import { useCallback, useReducer, useRef } from "react";
import { assertNever } from "../assert";
import type { AgentStatus, SessionKey } from "../types";
import { sessionKeyOf } from "../types";

export interface QueuedMessage {
	id: string;
	text: string;
}

export type QueueMoveDirection = "up" | "down";

export interface SessionQueues {
	items: QueuedMessage[];
	editingId: string | null;
	enqueue: (key: SessionKey, text: string) => void;
	remove: (key: SessionKey, id: string) => void;
	setText: (key: SessionKey, id: string, text: string) => void;
	setEditing: (key: SessionKey, id: string | null) => void;
	move: (key: SessionKey, id: string, direction: QueueMoveDirection) => void;
	takeNext: (key: SessionKey) => QueuedMessage | null;
	peek: (key: SessionKey) => {
		items: QueuedMessage[];
		editingId: string | null;
	};
}

export function useSessionQueues(
	selectedKey: SessionKey | null,
): SessionQueues {
	const [queues, dispatch] = useReducer(queueReducer, {});
	const queuesRef = useRef(queues);
	queuesRef.current = queues;

	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);
	const entry = selectedId === null ? undefined : queues[selectedId];
	const items = entry?.items ?? EMPTY;
	const editingId = entry?.editingId ?? null;

	const enqueue = useCallback((key: SessionKey, text: string) => {
		dispatch({
			type: "enqueue",
			sessionId: sessionKeyOf(key),
			message: { id: crypto.randomUUID(), text },
		});
	}, []);

	const remove = useCallback((key: SessionKey, id: string) => {
		dispatch({ type: "remove", sessionId: sessionKeyOf(key), id });
	}, []);

	const setText = useCallback((key: SessionKey, id: string, text: string) => {
		dispatch({ type: "setText", sessionId: sessionKeyOf(key), id, text });
	}, []);

	const setEditing = useCallback((key: SessionKey, id: string | null) => {
		dispatch({ type: "setEditing", sessionId: sessionKeyOf(key), id });
	}, []);

	const move = useCallback(
		(key: SessionKey, id: string, direction: QueueMoveDirection) => {
			dispatch({
				type: "move",
				sessionId: sessionKeyOf(key),
				id,
				direction,
			});
		},
		[],
	);

	const takeNext = useCallback((key: SessionKey) => {
		const sessionId = sessionKeyOf(key);
		const current = queuesRef.current[sessionId];
		const candidate = drainCandidate(
			current?.items ?? EMPTY,
			current?.editingId ?? null,
			"idle",
		);
		if (candidate !== null) {
			dispatch({ type: "remove", sessionId, id: candidate.id });
		}
		return candidate;
	}, []);

	const peek = useCallback((key: SessionKey) => {
		const current = queuesRef.current[sessionKeyOf(key)];
		return {
			items: current?.items ?? EMPTY,
			editingId: current?.editingId ?? null,
		};
	}, []);

	return {
		items,
		editingId,
		enqueue,
		remove,
		setText,
		setEditing,
		move,
		takeNext,
		peek,
	};
}

const EMPTY: QueuedMessage[] = [];

interface QueueEntry {
	items: QueuedMessage[];
	editingId: string | null;
}

export type Queues = Record<string, QueueEntry>;

export type QueueAction =
	| { type: "enqueue"; sessionId: string; message: QueuedMessage }
	| { type: "remove"; sessionId: string; id: string }
	| { type: "setText"; sessionId: string; id: string; text: string }
	| { type: "setEditing"; sessionId: string; id: string | null }
	| {
			type: "move";
			sessionId: string;
			id: string;
			direction: QueueMoveDirection;
	  };

export function drainCandidate(
	items: QueuedMessage[],
	editingId: string | null,
	status: AgentStatus,
): QueuedMessage | null {
	if (status !== "idle") return null;
	const head = items[0] ?? null;
	if (head === null) return null;
	if (editingId !== null && editingId === head.id) return null;
	return head;
}

export function moveQueuedItem(
	items: QueuedMessage[],
	id: string,
	direction: QueueMoveDirection,
): QueuedMessage[] | null {
	const index = items.findIndex((item) => item.id === id);
	if (index === -1) return null;
	const target = direction === "up" ? index - 1 : index + 1;
	if (target < 0 || target >= items.length) return null;
	const next = [...items];
	const current = next[index];
	const other = next[target];
	if (current === undefined || other === undefined) return null;
	next[index] = other;
	next[target] = current;
	return next;
}

export function queueReducer(state: Queues, action: QueueAction): Queues {
	switch (action.type) {
		case "enqueue": {
			const current = state[action.sessionId];
			return {
				...state,
				[action.sessionId]: {
					items: [...(current?.items ?? []), action.message],
					editingId: current?.editingId ?? null,
				},
			};
		}
		case "remove": {
			const current = state[action.sessionId];
			if (current === undefined) return state;
			const next = current.items.filter((item) => item.id !== action.id);
			if (next.length === current.items.length) return state;
			if (next.length === 0) {
				const { [action.sessionId]: _dropped, ...rest } = state;
				return rest;
			}
			return {
				...state,
				[action.sessionId]: { items: next, editingId: current.editingId },
			};
		}
		case "setText": {
			const current = state[action.sessionId];
			if (current === undefined) return state;
			let changed = false;
			const next = current.items.map((item) => {
				if (item.id !== action.id) return item;
				changed = true;
				return { ...item, text: action.text };
			});
			return changed
				? {
						...state,
						[action.sessionId]: { items: next, editingId: current.editingId },
					}
				: state;
		}
		case "setEditing": {
			const current = state[action.sessionId];
			if ((current?.editingId ?? null) === action.id) return state;
			return {
				...state,
				[action.sessionId]: {
					items: current?.items ?? [],
					editingId: action.id,
				},
			};
		}
		case "move": {
			const current = state[action.sessionId];
			if (current === undefined) return state;
			const next = moveQueuedItem(current.items, action.id, action.direction);
			if (next === null) return state;
			return {
				...state,
				[action.sessionId]: { items: next, editingId: current.editingId },
			};
		}
		default:
			return assertNever(action);
	}
}
