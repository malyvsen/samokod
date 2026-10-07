import { useEffect } from "react";
import { scopingTemplate } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";
import type { ChatState } from "./store";

export function useScopingPreview(
	selectedKey: SessionKey | null,
	updateChat: (key: SessionKey, next: (chat: ChatState) => ChatState) => void,
): void {
	useEffect(() => {
		if (selectedKey === null) return;
		const key = selectedKey;
		let cancelled = false;
		previewPromiseFor(key).then((template) => {
			if (cancelled || template === null) return;
			updateChat(key, (chat) =>
				chat.scopingPreview === template
					? chat
					: { ...chat, scopingPreview: template },
			);
		});
		return () => {
			cancelled = true;
		};
	}, [selectedKey, updateChat]);
}

export function previewPromiseFor(key: SessionKey): Promise<string | null> {
	if (key.role !== "scoping") return Promise.resolve(null);
	const id = sessionKeyOf(key);
	const cached = previews.get(id);
	if (cached !== undefined) return cached;
	const pending = scopingTemplate(key)
		.then((template) => template ?? null)
		.catch((error: unknown) => {
			console.warn("scoping_template failed", error);
			return null;
		});
	previews.set(id, pending);
	return pending;
}

/// Test-only reset for the module-level fetch cache.
export function clearPreviewCache(): void {
	previews.clear();
}

const previews = new Map<string, Promise<string | null>>();
