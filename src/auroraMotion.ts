import { useCallback, useEffect, useRef } from "react";

export const EDIT_STEP_MS = 180;
export const MAX_QUEUED_MS = EDIT_STEP_MS * 3;
export const EASE_TAU_MS = 160;
const MAX_FRAME_MS = 50;

export function useAuroraMotion(rootRef: React.RefObject<HTMLElement | null>) {
	const queuedRef = useRef(0);
	const tracksRef = useRef<Track[]>([]);

	const notifyEdit = useCallback(() => {
		if (reducedMotion()) return;
		queuedRef.current = queueEdit(queuedRef.current);
	}, []);

	useEffect(() => {
		if (rootRef.current === null) return;
		const root = rootRef.current;
		tracksRef.current = collectTracks(root);
		let frame = 0;
		let previous: number | null = null;
		const media = window.matchMedia("(prefers-reduced-motion: reduce)");
		function reset() {
			queuedRef.current = 0;
			previous = null;
		}
		media.addEventListener("change", reset);

		function tick(now: number) {
			frame = window.requestAnimationFrame(tick);
			if (tracksRef.current.length === 0) {
				tracksRef.current = collectTracks(root);
			}
			const elapsedMs =
				previous === null ? 0 : Math.min(now - previous, MAX_FRAME_MS);
			previous = now;
			if (reducedMotion()) {
				queuedRef.current = 0;
				return;
			}
			const released = releaseMotion(queuedRef.current, elapsedMs);
			queuedRef.current = released.queuedMs;
			const advanceMs = elapsedMs + released.advanceMs;
			if (advanceMs === 0) return;
			for (const track of tracksRef.current) {
				track.currentTime = Number(track.currentTime ?? 0) + advanceMs;
			}
		}
		frame = window.requestAnimationFrame(tick);
		return () => {
			window.cancelAnimationFrame(frame);
			media.removeEventListener("change", reset);
		};
	}, [rootRef]);

	return notifyEdit;
}

export function queueEdit(queuedMs: number): number {
	return Math.min(queuedMs + EDIT_STEP_MS, MAX_QUEUED_MS);
}

export function releaseMotion(
	queuedMs: number,
	elapsedMs: number,
): { advanceMs: number; queuedMs: number } {
	const advanceMs = queuedMs * (1 - Math.exp(-elapsedMs / EASE_TAU_MS));
	return { advanceMs, queuedMs: queuedMs - advanceMs };
}

export function reducedMotion(): boolean {
	return (
		typeof window !== "undefined" &&
		typeof window.matchMedia === "function" &&
		window.matchMedia("(prefers-reduced-motion: reduce)").matches
	);
}

type Track = Pick<Animation, "pause" | "currentTime">;

function collectTracks(root: HTMLElement): Track[] {
	const nodes = root.querySelectorAll(".aurora, .rays i");
	const tracks: Track[] = [];
	for (const node of nodes) {
		if (typeof node.getAnimations !== "function") continue;
		for (const animation of node.getAnimations()) {
			animation.pause();
			tracks.push(animation);
		}
	}
	return tracks;
}
