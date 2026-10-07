import { useCallback, useReducer, useRef } from "react";
import { assertNever } from "../assert";
import { type SessionKey, sessionKeyOf } from "../types";

export interface QueuedMessage {
	id: string;
	text: string;
}

export function useSessionQueues(selectedKey: SessionKey | null): {
	items: QueuedMessage[];
	enqueue: (key: SessionKey, text: string) => void;
	remove: (key: SessionKey, id: string) => void;
	setText: (key: SessionKey, id: string, text: string) => void;
	dequeueHead: (key: SessionKey) => QueuedMessage | null;
} {
	const [queues, dispatch] = useReducer(queueReducer, {});
	const queuesRef = useRef(queues);
	queuesRef.current = queues;

	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);
	const items = selectedId === null ? EMPTY : (queues[selectedId] ?? EMPTY);

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

	const dequeueHead = useCallback((key: SessionKey) => {
		const sessionId = sessionKeyOf(key);
		const head = queuesRef.current[sessionId]?.[0] ?? null;
		if (head !== null) {
			dispatch({ type: "remove", sessionId, id: head.id });
		}
		return head;
	}, []);

	return { items, enqueue, remove, setText, dequeueHead };
}

const EMPTY: QueuedMessage[] = [];

export type Queues = Record<string, QueuedMessage[]>;

export type QueueAction =
	| { type: "enqueue"; sessionId: string; message: QueuedMessage }
	| { type: "remove"; sessionId: string; id: string }
	| { type: "setText"; sessionId: string; id: string; text: string };

export function queueReducer(state: Queues, action: QueueAction): Queues {
	switch (action.type) {
		case "enqueue":
			return {
				...state,
				[action.sessionId]: [
					...(state[action.sessionId] ?? []),
					action.message,
				],
			};
		case "remove": {
			const current = state[action.sessionId];
			if (current === undefined) return state;
			const next = current.filter((item) => item.id !== action.id);
			if (next.length === current.length) return state;
			if (next.length === 0) {
				const { [action.sessionId]: _dropped, ...rest } = state;
				return rest;
			}
			return { ...state, [action.sessionId]: next };
		}
		case "setText": {
			const current = state[action.sessionId];
			if (current === undefined) return state;
			let changed = false;
			const next = current.map((item) => {
				if (item.id !== action.id) return item;
				changed = true;
				return { ...item, text: action.text };
			});
			return changed ? { ...state, [action.sessionId]: next } : state;
		}
		default:
			return assertNever(action);
	}
}
