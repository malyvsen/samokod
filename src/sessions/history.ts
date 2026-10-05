import { useEffect, useRef } from "react";
import { loadHistory } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

// History replays only for idle empty transcripts. A live turn owns its
// session pickers, so replays never run over a working key.
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
		const idleEmpty =
			(chat?.transcript.length ?? 0) === 0 &&
			!(chat?.historyLoading ?? false) &&
			(chat?.historyError ?? null) === null &&
			!(chat?.working ?? false);
		if (!idleEmpty) return;
		fired.current.add(id);
		loadHistory(selectedKey).catch((error: unknown) => {
			console.warn("load_history failed", error);
		});
	}, [selectedKey, chats]);
}
