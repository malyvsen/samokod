import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { testKey } from "../fixtures";
import type { SessionKey, TranscriptItem } from "../types";
import { type ChatState, emptyChat } from "./store";
import {
	BURST_SILENCE_MS,
	freezeBurst,
	hasRunningTools,
	liveForThought,
	secondsForBurst,
	useThoughtTimers,
	WAITING_DELAY_MS,
} from "./thoughts";

describe("thought timeouts", () => {
	test("burst window outlasts the waiting delay", () => {
		expect(BURST_SILENCE_MS).toBeGreaterThan(WAITING_DELAY_MS);
	});
});

describe("secondsForBurst", () => {
	test("single-chunk bursts read one second", () => {
		expect(secondsForBurst(1000, 1000)).toBe(1);
		expect(secondsForBurst(1000, 1400)).toBe(1);
	});

	test("rounds to the nearest second with a one-second floor", () => {
		expect(secondsForBurst(0, 8000)).toBe(8);
		expect(secondsForBurst(0, 1500)).toBe(2);
		expect(secondsForBurst(0, 1499)).toBe(1);
	});
});

describe("liveForThought", () => {
	test("starts a burst from waiting or idle", () => {
		expect(liveForThought(null, "hello", 1000)).toEqual({
			kind: "thinking",
			tail: "hello",
			burstStart: 1000,
			updatedAt: 1000,
		});
		expect(liveForThought({ kind: "waiting" }, "hello", 1000)).toEqual({
			kind: "thinking",
			tail: "hello",
			burstStart: 1000,
			updatedAt: 1000,
		});
	});

	test("appends within the burst window", () => {
		const live = liveForThought(null, "hello", 1000);
		expect(liveForThought(live, " world", 2500)).toEqual({
			kind: "thinking",
			tail: "hello world",
			burstStart: 1000,
			updatedAt: 2500,
		});
	});

	test("starts fresh past burst silence", () => {
		const live = liveForThought(null, "hello", 1000);
		expect(liveForThought(live, "again", 1000 + BURST_SILENCE_MS)).toEqual({
			kind: "thinking",
			tail: "again",
			burstStart: 1000 + BURST_SILENCE_MS,
			updatedAt: 1000 + BURST_SILENCE_MS,
		});
	});
});

describe("freezeBurst", () => {
	test("freezes thinking into a trace and drops the tail", () => {
		const transcript = freezeBurst([], liveForThought(null, "hello", 1000));
		expect(transcript).toHaveLength(1);
		expect(transcript[0]).toMatchObject({ kind: "thought", seconds: 1 });
	});

	test("leaves waiting and idle untouched", () => {
		expect(freezeBurst([], null)).toEqual([]);
		expect(freezeBurst([], { kind: "waiting" })).toEqual([]);
	});
});

describe("hasRunningTools", () => {
	test("empty transcript has nothing running", () => {
		expect(hasRunningTools([])).toBe(false);
	});

	test("pending and in-progress tools count as running", () => {
		const pending: TranscriptItem[] = [
			{
				kind: "tool",
				id: "t1",
				line: { id: "l1", text: "edit", status: "pending" },
			},
		];
		const running: TranscriptItem[] = [
			{
				kind: "tool",
				id: "t1",
				line: { id: "l1", text: "edit", status: "in_progress" },
			},
		];
		expect(hasRunningTools(pending)).toBe(true);
		expect(hasRunningTools(running)).toBe(true);
	});

	test("completed mix with no running tools reads idle", () => {
		const transcript: TranscriptItem[] = [
			{
				kind: "tool",
				id: "t1",
				line: { id: "l1", text: "edit", status: "completed" },
			},
			{ kind: "agent", id: "a1", text: "done" },
		];
		expect(hasRunningTools(transcript)).toBe(false);
	});

	test("failed only reads idle", () => {
		const transcript: TranscriptItem[] = [
			{
				kind: "tool",
				id: "t1",
				line: { id: "l1", text: "edit", status: "failed" },
			},
		];
		expect(hasRunningTools(transcript)).toBe(false);
	});
});

describe("useThoughtTimers waiting gate", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	function runWaitingTimer(chat: ChatState) {
		const updateChat = vi.fn(
			(_key: SessionKey, _next: (current: ChatState) => ChatState) => {},
		);
		const hook = renderHook(
			({ current }: { current: ChatState }) =>
				useThoughtTimers(testKey(), current, updateChat),
			{ initialProps: { current: chat } },
		);
		act(() => {
			vi.advanceTimersByTime(WAITING_DELAY_MS);
		});
		const updater = updateChat.mock.calls[0]?.[1] as
			| ((current: ChatState) => ChatState)
			| undefined;
		const next = updater?.(chat) ?? chat;
		hook.unmount();
		return next;
	}

	test("quiet working turn flips to waiting", () => {
		const chat: ChatState = { ...emptyChat(), working: true, live: null };
		expect(runWaitingTimer(chat).live).toEqual({ kind: "waiting" });
	});

	test("running tool stays quiet past the delay", () => {
		const chat: ChatState = {
			...emptyChat(),
			working: true,
			live: null,
			transcript: [
				{
					kind: "tool",
					id: "t1",
					line: { id: "l1", text: "edit", status: "in_progress" },
				},
			],
		};
		expect(runWaitingTimer(chat).live).toBeNull();
	});

	test("pending approval never flips to waiting", () => {
		const chat: ChatState = {
			...emptyChat(),
			working: true,
			approval: true,
			live: null,
		};
		expect(runWaitingTimer(chat).live).toBeNull();
	});
});
