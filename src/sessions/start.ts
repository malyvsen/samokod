import { useEffect, useRef } from "react";
import { loadHistory, warmSession } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

// Single owner for session start: empty or unknown keys load history once
// (backend emits preparing, then replaying only with past); idle
// non-empty keys without live pickers warm once. Never both for one key.
export function useSessionStart(
	selectedKey: SessionKey | null,
	chats: Chats,
	readOnly: boolean,
): void {
	const fired = useRef<Set<string>>(new Set());
	useEffect(() => {
		if (selectedKey === null || readOnly) return;
		const id = sessionKeyOf(selectedKey);
		if (fired.current.has(id)) return;
		const chat = chats[id];
		if (chat === undefined || chat.transcript.length === 0) {
			const idleEmpty =
				chat === undefined || (chat.start.kind === "idle" && !chat.working);
			if (!idleEmpty) return;
			fired.current.add(id);
			loadHistory(selectedKey).catch((error: unknown) => {
				console.warn("load_history failed", error);
			});
			return;
		}
		const needsWarm =
			chat.start.kind === "idle" &&
			!chat.working &&
			chat.configOptions.length === 0;
		if (!needsWarm) return;
		fired.current.add(id);
		warmSession(selectedKey).catch((error: unknown) => {
			console.warn("warm_session failed", error);
		});
	}, [selectedKey, chats, readOnly]);
}
