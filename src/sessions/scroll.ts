import { useCallback, useLayoutEffect, useRef } from "react";

export function usePinnedTranscript(
	selectedId: string | null,
	transcript: unknown[],
) {
	const scrollRef = useRef<HTMLDivElement | null>(null);
	const pinnedRef = useRef(true);
	const lastSelectedRef = useRef<string | null>(null);

	const onScroll = useCallback(() => {
		const node = scrollRef.current;
		if (node === null) return;
		pinnedRef.current = isPinned({
			scrollHeight: node.scrollHeight,
			scrollTop: node.scrollTop,
			clientHeight: node.clientHeight,
		});
	}, []);

	useLayoutEffect(() => {
		// Reading transcript keeps the identity dep honest: new chunks re-run the effect.
		void transcript;
		const selectionChanged = selectedId !== lastSelectedRef.current;
		lastSelectedRef.current = selectedId;
		const node = scrollRef.current;
		if (node === null) return;
		if (shouldAutoScroll(pinnedRef.current, selectionChanged)) {
			node.scrollTop = node.scrollHeight;
			pinnedRef.current = true;
		}
	}, [transcript, selectedId]);

	return { scrollRef, onScroll };
}

// Covers fractional pixels + a line of text without masking intent.
const NEAR_BOTTOM_PX = 48;

export function isPinned(metrics: {
	scrollHeight: number;
	scrollTop: number;
	clientHeight: number;
}) {
	return (
		metrics.scrollHeight - metrics.scrollTop - metrics.clientHeight <=
		NEAR_BOTTOM_PX
	);
}

export function shouldAutoScroll(pinned: boolean, selectionChanged: boolean) {
	return pinned || selectionChanged;
}
