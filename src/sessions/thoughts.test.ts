import { describe, expect, test } from "vitest";
import { accumulateThought, freezeThought, thinkingSeconds } from "./thoughts";

describe("thinkingSeconds", () => {
	test("single-chunk thoughts read one second", () => {
		expect(thinkingSeconds(0)).toBe(1);
		expect(thinkingSeconds(400)).toBe(1);
	});

	test("rounds to the nearest second with a one-second floor", () => {
		expect(thinkingSeconds(8000)).toBe(8);
		expect(thinkingSeconds(1500)).toBe(2);
		expect(thinkingSeconds(1499)).toBe(1);
	});
});

describe("accumulateThought", () => {
	test("starts thinking from idle", () => {
		expect(accumulateThought(null, 1000, "hmm")).toEqual({
			startedAt: 1000,
			updatedAt: 1000,
			text: "hmm",
		});
	});

	test("accumulates chunks into one thought text", () => {
		const live = accumulateThought(null, 1000, "hel");
		expect(accumulateThought(live, 2500, "lo")).toEqual({
			startedAt: 1000,
			updatedAt: 2500,
			text: "hello",
		});
	});
});

describe("freezeThought", () => {
	test("freezes thinking with text", () => {
		const transcript = freezeThought(
			[],
			accumulateThought(null, 1000, "hmm"),
			2000,
		);
		expect(transcript).toHaveLength(1);
		expect(transcript[0]).toMatchObject({
			kind: "thought",
			ms: 0,
			text: "hmm",
			at: 2000,
		});
	});

	test("stores accumulated text", () => {
		const live = accumulateThought(
			accumulateThought(null, 1000, "hel"),
			2500,
			"lo",
		);
		const transcript = freezeThought([], live, 3000);
		expect(transcript[0]).toMatchObject({
			kind: "thought",
			ms: 1500,
			text: "hello",
			at: 3000,
		});
	});

	test("leaves idle untouched", () => {
		expect(freezeThought([], null, 1000)).toEqual([]);
	});
});
