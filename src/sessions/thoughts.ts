import type { TranscriptItem } from "../types";

export type LiveStatus = {
	kind: "thinking";
	burstStart: number;
	updatedAt: number;
	text: string;
};

export function liveForThought(
	live: LiveStatus | null,
	now: number,
	chunk = "",
): LiveStatus {
	if (live !== null) {
		return {
			kind: "thinking",
			burstStart: live.burstStart,
			updatedAt: now,
			text: live.text + chunk,
		};
	}
	return { kind: "thinking", burstStart: now, updatedAt: now, text: chunk };
}

export function freezeBurst(
	transcript: TranscriptItem[],
	live: LiveStatus | null,
	now: number = Date.now(),
): TranscriptItem[] {
	if (live === null) return transcript;
	return [
		...transcript,
		{
			kind: "thought",
			id: crypto.randomUUID(),
			ms: burstMs(live.burstStart, live.updatedAt),
			text: live.text,
			at: now,
		},
	];
}

export function thinkingSeconds(totalMs: number): number {
	return Math.max(1, Math.round(totalMs / 1000));
}

function burstMs(burstStart: number, updatedAt: number): number {
	return Math.max(0, updatedAt - burstStart);
}
