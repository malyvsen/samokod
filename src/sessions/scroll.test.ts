import { renderHook } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { isPinned, shouldAutoScroll, usePinnedTranscript } from "./scroll";

function fakeNode(overrides: {
	scrollHeight: number;
	scrollTop: number;
	clientHeight: number;
}) {
	return { ...overrides };
}

describe("isPinned", () => {
	test("pinned at the bottom", () => {
		expect(
			isPinned({ scrollHeight: 1000, scrollTop: 500, clientHeight: 500 }),
		).toBe(true);
	});

	test("pinned exactly at the threshold", () => {
		expect(
			isPinned({ scrollHeight: 1000, scrollTop: 452, clientHeight: 500 }),
		).toBe(true);
	});

	test("unpinned one pixel past the threshold", () => {
		expect(
			isPinned({ scrollHeight: 1000, scrollTop: 451, clientHeight: 500 }),
		).toBe(false);
	});

	test("unpinned far from the bottom", () => {
		expect(
			isPinned({ scrollHeight: 1000, scrollTop: 0, clientHeight: 500 }),
		).toBe(false);
	});

	test("fractional pixels stay pinned inside the threshold", () => {
		expect(
			isPinned({ scrollHeight: 1000.4, scrollTop: 452.5, clientHeight: 500 }),
		).toBe(true);
	});
});

describe("shouldAutoScroll", () => {
	test("truth table", () => {
		expect(shouldAutoScroll(true, false)).toBe(true);
		expect(shouldAutoScroll(true, true)).toBe(true);
		expect(shouldAutoScroll(false, true)).toBe(true);
		expect(shouldAutoScroll(false, false)).toBe(false);
	});
});

describe("usePinnedTranscript", () => {
	test("pinned stays pinned on new transcript", () => {
		const { result, rerender } = renderHook(
			({
				selectedId,
				transcript,
			}: {
				selectedId: string;
				transcript: unknown[];
			}) => usePinnedTranscript(selectedId, transcript),
			{ initialProps: { selectedId: "a", transcript: [] as unknown[] } },
		);
		const node = fakeNode({
			scrollHeight: 1000,
			scrollTop: 500,
			clientHeight: 500,
		});
		result.current.scrollRef.current = node as unknown as HTMLDivElement;
		result.current.onScroll();
		node.scrollHeight = 1200;
		rerender({ selectedId: "a", transcript: [{ id: 1 }] });
		expect(node.scrollTop).toBe(1200);
	});

	test("unpinned holds position on new transcript", () => {
		const { result, rerender } = renderHook(
			({
				selectedId,
				transcript,
			}: {
				selectedId: string;
				transcript: unknown[];
			}) => usePinnedTranscript(selectedId, transcript),
			{ initialProps: { selectedId: "a", transcript: [] as unknown[] } },
		);
		const node = fakeNode({
			scrollHeight: 1000,
			scrollTop: 0,
			clientHeight: 500,
		});
		result.current.scrollRef.current = node as unknown as HTMLDivElement;
		result.current.onScroll();
		node.scrollHeight = 1200;
		rerender({ selectedId: "a", transcript: [{ id: 1 }] });
		expect(node.scrollTop).toBe(0);
	});

	test("selection change forces scroll even when unpinned", () => {
		const { result, rerender } = renderHook(
			({
				selectedId,
				transcript,
			}: {
				selectedId: string;
				transcript: unknown[];
			}) => usePinnedTranscript(selectedId, transcript),
			{ initialProps: { selectedId: "a", transcript: [] as unknown[] } },
		);
		const node = fakeNode({
			scrollHeight: 1000,
			scrollTop: 0,
			clientHeight: 500,
		});
		result.current.scrollRef.current = node as unknown as HTMLDivElement;
		result.current.onScroll();
		node.scrollHeight = 1200;
		rerender({ selectedId: "b", transcript: [{ id: 1 }] });
		expect(node.scrollTop).toBe(1200);
	});
});
