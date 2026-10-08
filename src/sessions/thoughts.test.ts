import { describe, expect, test } from "vitest";
import {
	BURST_SILENCE_MS,
	freezeBurst,
	liveForThought,
	secondsForBurst,
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
