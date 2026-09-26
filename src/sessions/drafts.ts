import { useCallback, useEffect, useRef, useState } from "react";
import { scopingDraft } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";
import type { Chats } from "./store";

export function hasUserMessage(chats: Chats, key: SessionKey): boolean {
	return (
		chats[sessionKeyOf(key)]?.transcript.some((item) => item.kind === "user") ??
		false
	);
}

export function useSessionDrafts(
	selectedKey: SessionKey | null,
	chats: Chats,
): {
	initialText: string | undefined;
	ready: boolean;
	onDraftInput: (text: string) => void;
	onDraftSent: () => void;
} {
	const [drafts, setDrafts] = useState<Record<string, string>>({});
	const attempted = useRef<Set<string>>(new Set());
	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);

	const [readyId, setReadyId] = useState<string | null>(null);

	useEffect(() => {
		if (selectedKey === null) return;
		const id = sessionKeyOf(selectedKey);
		if (
			!attempted.current.has(id) &&
			selectedKey.role === "scoping" &&
			!hasUserMessage(chats, selectedKey)
		) {
			attempted.current.add(id);
			let cancelled = false;
			scopingDraft(selectedKey)
				.then((draft) => {
					if (cancelled) return;
					if (draft !== null) {
						setDrafts((current) =>
							current[id] === undefined ? { ...current, [id]: draft } : current,
						);
					}
					setReadyId(id);
				})
				.catch((error: unknown) => {
					console.warn("scoping_draft failed", error);
					if (!cancelled) setReadyId(id);
				});
			return () => {
				cancelled = true;
			};
		}
		attempted.current.add(id);
		setReadyId(id);
	}, [selectedKey, chats]);

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
	const ready = selectedId !== null && readyId === selectedId;
	return { initialText, ready, onDraftInput, onDraftSent };
}
