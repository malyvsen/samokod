import { useCallback, useRef, useState } from "react";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

export function hasSentUserMessage(chats: Chats, key: SessionKey): boolean {
	return (
		chats[sessionKeyOf(key)]?.transcript.some((item) => item.kind === "user") ??
		false
	);
}

export function isBlankDraft(text: string | null | undefined): boolean {
	return (text ?? "").trim() === "";
}

export function shouldKeepOnSwitch(
	chats: Chats,
	drafts: Record<string, string>,
	key: SessionKey,
): boolean {
	if (hasSentUserMessage(chats, key)) return true;
	return !isBlankDraft(drafts[sessionKeyOf(key)]);
}

// Per-session compose-box drafts: unsent text kept per session so
// switching sessions never loses what was typed.
export function useSessionDrafts(selectedKey: SessionKey | null): {
	initialText: string | undefined;
	onDraftInput: (text: string) => void;
	onDraftSent: () => void;
	drafts: Record<string, string>;
	draftFor: (key: SessionKey) => string | undefined;
	clearDraft: (key: SessionKey) => void;
} {
	const [drafts, setDrafts] = useState<Record<string, string>>({});
	const draftsRef = useRef<Record<string, string>>({});
	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);

	const onDraftInput = useCallback(
		(text: string) => {
			if (selectedId === null) return;
			const id = selectedId;
			draftsRef.current = { ...draftsRef.current, [id]: text };
			setDrafts((current) =>
				current[id] === text ? current : { ...current, [id]: text },
			);
		},
		[selectedId],
	);

	const onDraftSent = useCallback(() => {
		if (selectedId === null) return;
		const id = selectedId;
		if (draftsRef.current[id] === undefined) return;
		const next = { ...draftsRef.current };
		delete next[id];
		draftsRef.current = next;
		setDrafts((current) => {
			if (current[id] === undefined) return current;
			const next = { ...current };
			delete next[id];
			return next;
		});
	}, [selectedId]);

	const draftFor = useCallback((key: SessionKey): string | undefined => {
		return draftsRef.current[sessionKeyOf(key)];
	}, []);

	const clearDraft = useCallback((key: SessionKey) => {
		const id = sessionKeyOf(key);
		if (draftsRef.current[id] !== undefined) {
			const next = { ...draftsRef.current };
			delete next[id];
			draftsRef.current = next;
		}
		setDrafts((current) => {
			if (current[id] === undefined) return current;
			const next = { ...current };
			delete next[id];
			return next;
		});
	}, []);

	const initialText = selectedId === null ? undefined : drafts[selectedId];
	return {
		initialText,
		onDraftInput,
		onDraftSent,
		drafts,
		draftFor,
		clearDraft,
	};
}
