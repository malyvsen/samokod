import { useCallback, useState } from "react";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

export function hasUserMessage(chats: Chats, key: SessionKey): boolean {
	return (
		chats[sessionKeyOf(key)]?.transcript.some((item) => item.kind === "user") ??
		false
	);
}

// Per-session compose drafts for user-typed text only. The compose box
// mounts empty; the scoping template never prefills and is instead fetched
// lazily at send time for display.
export function useSessionDrafts(
	selectedKey: SessionKey | null,
	_chats: Chats,
): {
	initialText: string | undefined;
	onDraftInput: (text: string) => void;
	onDraftSent: () => void;
} {
	const [drafts, setDrafts] = useState<Record<string, string>>({});
	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);

	const onDraftInput = useCallback(
		(text: string) => {
			if (selectedId === null) return;
			const id = selectedId;
			setDrafts((current) =>
				current[id] === text ? current : { ...current, [id]: text },
			);
		},
		[selectedId],
	);

	const onDraftSent = useCallback(() => {
		if (selectedId === null) return;
		const id = selectedId;
		setDrafts((current) => {
			if (current[id] === undefined) return current;
			const next = { ...current };
			delete next[id];
			return next;
		});
	}, [selectedId]);

	const initialText = selectedId === null ? undefined : drafts[selectedId];
	return { initialText, onDraftInput, onDraftSent };
}
