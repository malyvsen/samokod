import type { TranscriptItem } from "../types";

export type LiveThought = {
	startedAt: number;
	updatedAt: number;
	text: string;
};

export function accumulateThought(
	live: LiveThought | null,
	now: number,
	chunk = "",
): LiveThought {
	if (live !== null) {
		return {
			startedAt: live.startedAt,
			updatedAt: now,
			text: live.text + chunk,
		};
	}
	return { startedAt: now, updatedAt: now, text: chunk };
}

export function freezeThought(
	transcript: TranscriptItem[],
	live: LiveThought | null,
	now: number,
): TranscriptItem[] {
	if (live === null) return transcript;
	return [
		...transcript,
		{
			kind: "thought",
			id: crypto.randomUUID(),
			ms: thoughtMs(live.startedAt, live.updatedAt),
			text: live.text,
			at: now,
		},
	];
}

export function thinkingSeconds(totalMs: number): number {
	return Math.max(1, Math.round(totalMs / 1000));
}

function thoughtMs(startedAt: number, updatedAt: number): number {
	return Math.max(0, updatedAt - startedAt);
}
