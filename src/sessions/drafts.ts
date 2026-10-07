import { useCallback, useState } from "react";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

export function hasUserMessage(chats: Chats, key: SessionKey): boolean {
	return (
		chats[sessionKeyOf(key)]?.transcript.some((item) => item.kind === "user") ??
		false
	);
}

// Per-session compose-box drafts: unsent text kept per session so
// switching sessions never loses what was typed.
export function useSessionDrafts(selectedKey: SessionKey | null): {
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
