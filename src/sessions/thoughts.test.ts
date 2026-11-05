import { describe, expect, test } from "vitest";
import { freezeBurst, liveForThought, thinkingSeconds } from "./thoughts";

describe("thinkingSeconds", () => {
	test("single-chunk bursts read one second", () => {
		expect(thinkingSeconds(0)).toBe(1);
		expect(thinkingSeconds(400)).toBe(1);
	});

	test("rounds to the nearest second with a one-second floor", () => {
		expect(thinkingSeconds(8000)).toBe(8);
		expect(thinkingSeconds(1500)).toBe(2);
		expect(thinkingSeconds(1499)).toBe(1);
	});
});

describe("liveForThought", () => {
	test("starts a burst from idle", () => {
		expect(liveForThought(null, 1000, "hmm")).toEqual({
			kind: "thinking",
			burstStart: 1000,
			updatedAt: 1000,
			text: "hmm",
		});
	});

	test("accumulates chunks into one burst text", () => {
		const live = liveForThought(null, 1000, "hel");
		expect(liveForThought(live, 2500, "lo")).toEqual({
			kind: "thinking",
			burstStart: 1000,
			updatedAt: 2500,
			text: "hello",
		});
	});
});

describe("freezeBurst", () => {
	test("freezes thinking with text", () => {
		const transcript = freezeBurst([], liveForThought(null, 1000, "hmm"));
		expect(transcript).toHaveLength(1);
		expect(transcript[0]).toMatchObject({
			kind: "thought",
			ms: 0,
			text: "hmm",
		});
	});

	test("stores accumulated text", () => {
		const live = liveForThought(liveForThought(null, 1000, "hel"), 2500, "lo");
		const transcript = freezeBurst([], live);
		expect(transcript[0]).toMatchObject({
			kind: "thought",
			ms: 1500,
			text: "hello",
		});
	});

	test("leaves idle untouched", () => {
		expect(freezeBurst([], null)).toEqual([]);
	});
});
