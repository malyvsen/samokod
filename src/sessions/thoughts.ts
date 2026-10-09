import { useEffect } from "react";
import type { SessionKey, TranscriptItem } from "../types";
import type { ChatState } from "./store";

export type LiveStatus =
	| { kind: "waiting" }
	| { kind: "thinking"; tail: string; burstStart: number; updatedAt: number };

export const BURST_SILENCE_MS = 3000;
export const WAITING_DELAY_MS = 2000;

export function liveForThought(
	live: LiveStatus | null,
	chunk: string,
	now: number,
): LiveStatus {
	if (live !== null && live.kind === "thinking") {
		if (now - live.updatedAt >= BURST_SILENCE_MS) {
			return { kind: "thinking", tail: chunk, burstStart: now, updatedAt: now };
		}
		return {
			kind: "thinking",
			tail: live.tail + chunk,
			burstStart: live.burstStart,
			updatedAt: now,
		};
	}
	return { kind: "thinking", tail: chunk, burstStart: now, updatedAt: now };
}

export function freezeBurst(
	transcript: TranscriptItem[],
	live: LiveStatus | null,
): TranscriptItem[] {
	if (live === null || live.kind !== "thinking") return transcript;
	const seconds = secondsForBurst(live.burstStart, live.updatedAt);
	return [...transcript, { kind: "thought", id: crypto.randomUUID(), seconds }];
}

export function secondsForBurst(burstStart: number, updatedAt: number): number {
	return Math.max(1, Math.round((updatedAt - burstStart) / 1000));
}

export function hasRunningTools(transcript: TranscriptItem[]): boolean {
	return transcript.some(
		(item) =>
			item.kind === "tool" &&
			(item.line.status === "pending" || item.line.status === "in_progress"),
	);
}

export function useThoughtTimers(
	selectedKey: SessionKey | null,
	chat: ChatState | null,
	updateChat: (key: SessionKey, next: (chat: ChatState) => ChatState) => void,
): void {
	const live = chat?.live ?? null;
	const working = chat?.working ?? false;
	const transcript = chat?.transcript;
	const approval = chat?.approval ?? false;
	// biome-ignore lint/correctness/useExhaustiveDependencies: transcript and approval re-arm the quiet gap; values come from current inside the timeout.
	useEffect(() => {
		if (selectedKey === null) return;
		const key = selectedKey;
		if (live !== null && !working) {
			updateChat(key, (current) => {
				if (current.live === null) return current;
				return {
					...current,
					transcript: freezeBurst(current.transcript, current.live),
					live: null,
				};
			});
			return;
		}
		if (live !== null && live.kind === "thinking") {
			const burstStart = live.burstStart;
			const updatedAt = live.updatedAt;
			const timer = setTimeout(() => {
				const now = Date.now();
				updateChat(key, (current) => {
					const currentLive = current.live;
					if (
						currentLive === null ||
						currentLive.kind !== "thinking" ||
						currentLive.updatedAt !== updatedAt
					) {
						return current;
					}
					if (now - updatedAt < BURST_SILENCE_MS) return current;
					const seconds = secondsForBurst(burstStart, updatedAt);
					const nextTranscript: TranscriptItem[] = [
						...current.transcript,
						{ kind: "thought", id: crypto.randomUUID(), seconds },
					];
					const waiting =
						current.working &&
						!current.approval &&
						!hasRunningTools(current.transcript);
					return {
						...current,
						transcript: nextTranscript,
						live: waiting ? { kind: "waiting" } : null,
					};
				});
			}, BURST_SILENCE_MS);
			return () => {
				clearTimeout(timer);
			};
		}
		if (live === null && working) {
			const timer = setTimeout(() => {
				updateChat(key, (current) => {
					if (current.live !== null || !current.working) return current;
					if (current.approval || hasRunningTools(current.transcript)) {
						return current;
					}
					return { ...current, live: { kind: "waiting" } };
				});
			}, WAITING_DELAY_MS);
			return () => {
				clearTimeout(timer);
			};
		}
		return;
	}, [selectedKey, live, working, transcript, approval, updateChat]);
}
