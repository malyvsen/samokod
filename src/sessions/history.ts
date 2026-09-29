import { useEffect, useRef } from "react";
import { loadHistory } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

export function useSessionHistory(
	selectedKey: SessionKey | null,
	chats: Chats,
): void {
	const fired = useRef<Set<string>>(new Set());
	useEffect(() => {
		if (selectedKey === null) return;
		const id = sessionKeyOf(selectedKey);
		if (fired.current.has(id)) return;
		const chat = chats[id];
		const isEmpty = (chat?.transcript.length ?? 0) === 0;
		const isLoading = chat?.historyLoading ?? false;
		const hasError = (chat?.historyError ?? null) !== null;
		if (!isEmpty || isLoading || hasError) return;
		fired.current.add(id);
		loadHistory(selectedKey).catch((error: unknown) => {
			console.warn("load_history failed", error);
		});
	}, [selectedKey, chats]);
}
