import { useEffect, useRef } from "react";
import { warmSession } from "../api";
import { type SessionKey, sessionKeyOf } from "../types";

export function useWarmSession(
	selectedKey: SessionKey | null,
	isLive: boolean,
	readOnly: boolean,
): void {
	const fired = useRef<Set<string>>(new Set());
	useEffect(() => {
		if (selectedKey === null || isLive || readOnly) return;
		const id = sessionKeyOf(selectedKey);
		if (fired.current.has(id)) return;
		fired.current.add(id);
		warmSession(selectedKey).catch((error: unknown) => {
			console.warn("warm_session failed", error);
		});
	}, [selectedKey, isLive, readOnly]);
}
