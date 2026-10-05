import { useEffect, useRef } from "react";
import { warmSession } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

// Warms idle non-empty sessions so pickers turn live before the first
// prompt. Empty transcripts skip: their history replay implies the warm.
// Working keys skip: the live turn owns its pickers.
export function useWarmSession(
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
		const isLive = (chat?.configOptions.length ?? 0) > 0;
		const needsHistory = (chat?.transcript.length ?? 0) === 0;
		const isWorking = chat?.working ?? false;
		if (isLive || needsHistory || isWorking) return;
		fired.current.add(id);
		warmSession(selectedKey).catch((error: unknown) => {
			console.warn("warm_session failed", error);
		});
	}, [selectedKey, chats, readOnly]);
}
