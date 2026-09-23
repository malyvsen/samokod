import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import {
	EASE_TAU_MS,
	EDIT_STEP_MS,
	MAX_QUEUED_MS,
	queueEdit,
	releaseMotion,
	useAuroraMotion,
} from "./auroraMotion";

describe("queued edits", () => {
	test("a single edit eases out instead of jumping", () => {
		let queued = queueEdit(0);
		expect(queued).toBe(EDIT_STEP_MS);
		const first = releaseMotion(queued, 16);
		expect(first.advanceMs).toBeGreaterThan(0);
		expect(first.advanceMs).toBeLessThan(queued);
		queued = first.queuedMs;
		const second = releaseMotion(queued, 16);
		expect(second.advanceMs).toBeGreaterThan(0);
		expect(second.advanceMs).toBeLessThan(first.advanceMs);
	});

	test("rapid edits cannot exceed the queue cap", () => {
		let queued = 0;
		for (let i = 0; i < 10; i += 1) {
			queued = queueEdit(queued);
		}
		expect(queued).toBe(MAX_QUEUED_MS);
	});

	test("split frames ease the same as one whole frame", () => {
		const whole = releaseMotion(EDIT_STEP_MS, 32);
		const half = releaseMotion(EDIT_STEP_MS, 16);
		const rest = releaseMotion(half.queuedMs, 16);
		expect(whole.queuedMs).toBeCloseTo(rest.queuedMs, 8);
		expect(whole.advanceMs).toBeCloseTo(half.advanceMs + rest.advanceMs, 8);
	});

	test("a long frame settles promptly instead of jumping", () => {
		const { advanceMs, queuedMs } = releaseMotion(MAX_QUEUED_MS, 50);
		const expected = MAX_QUEUED_MS * (1 - Math.exp(-50 / EASE_TAU_MS));
		expect(advanceMs).toBeCloseTo(expected, 8);
		expect(queuedMs).toBeGreaterThan(0);
	});
});

describe("useAuroraMotion", () => {
	let frames: FrameRequestCallback[];
	let cancelled: number[];

	beforeEach(() => {
		frames = [];
		cancelled = [];
		let nextId = 1;
		vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
			const id = nextId;
			nextId += 1;
			frames.push(callback);
			return id;
		});
		vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
			cancelled.push(id);
		});
		setReducedMotion(false);
	});

	afterEach(() => {
		vi.restoreAllMocks();
	});

	function setReducedMotion(matches: boolean) {
		Object.defineProperty(window, "matchMedia", {
			configurable: true,
			writable: true,
			value: () => ({
				matches,
				addEventListener: () => {},
				removeEventListener: () => {},
			}),
		});
	}

	function step(time: number) {
		const pending = [...frames];
		frames = [];
		for (const frame of pending) frame(time);
	}

	function mountAurora(currentTime: number) {
		const animation = {
			pause: () => {},
			currentTime: currentTime as number | null,
		};
		const root = document.createElement("div");
		const node = document.createElement("div");
		node.className = "aurora";
		(
			node as unknown as { getAnimations: () => (typeof animation)[] }
		).getAnimations = () => [animation];
		root.appendChild(node);
		const rootRef = { current: root } as React.RefObject<HTMLElement | null>;
		const hook = renderHook(() => useAuroraMotion(rootRef));
		return { animation, hook };
	}

	test("edits advance animation time and unmount cancels the frame", () => {
		const { animation, hook } = mountAurora(1000);
		step(0);
		expect(animation.currentTime).toBe(1000);

		act(() => {
			hook.result.current();
		});
		step(16);
		const eased = EDIT_STEP_MS * (1 - Math.exp(-16 / EASE_TAU_MS));
		expect(animation.currentTime ?? 0).toBeCloseTo(1000 + 16 + eased, 6);

		hook.unmount();
		expect(cancelled.length).toBeGreaterThan(0);
	});

	test("reduced motion discards queued movement", () => {
		const { animation, hook } = mountAurora(1000);
		act(() => {
			hook.result.current();
		});
		setReducedMotion(true);
		step(0);
		step(16);
		expect(animation.currentTime).toBe(1000);
	});
});
