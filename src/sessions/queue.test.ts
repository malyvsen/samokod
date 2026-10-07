import { renderHook } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { testKey } from "../fixtures";
import { drainCandidate, queueReducer, useSessionQueues } from "./queue";

describe("queueReducer", () => {
	test("enqueue appends per session", () => {
		const next = queueReducer(
			{},
			{
				type: "enqueue",
				sessionId: "a",
				message: { id: "1", text: "hello" },
			},
		);
		expect(next.a?.items).toEqual([{ id: "1", text: "hello" }]);
		expect(next.a?.editingId).toBeNull();
	});

	test("remove drops the item", () => {
		const start = {
			a: { items: [{ id: "1", text: "hello" }], editingId: null },
		};
		const next = queueReducer(start, {
			type: "remove",
			sessionId: "a",
			id: "1",
		});
		expect(next.a).toBeUndefined();
	});

	test("setText rewrites the item", () => {
		const start = {
			a: { items: [{ id: "1", text: "hello" }], editingId: null },
		};
		const next = queueReducer(start, {
			type: "setText",
			sessionId: "a",
			id: "1",
			text: "world",
		});
		expect(next.a?.items).toEqual([{ id: "1", text: "world" }]);
	});

	test("setEditing tracks the edited item", () => {
		const start = {
			a: { items: [{ id: "1", text: "hello" }], editingId: null },
		};
		const next = queueReducer(start, {
			type: "setEditing",
			sessionId: "a",
			id: "1",
		});
		expect(next.a?.editingId).toBe("1");
	});

	test("unknown session yields empty items", () => {
		const { result } = renderHook(() => useSessionQueues(testKey()));
		expect(result.current.items).toEqual([]);
		expect(result.current.editingId).toBeNull();
	});
});

describe("drainCandidate", () => {
	const head = { id: "1", text: "first" };
	const second = { id: "2", text: "second" };
	test("idle with head and no edit drains", () => {
		expect(drainCandidate([head], null, "idle")).toEqual(head);
	});
	test("idle empty never drains", () => {
		expect(drainCandidate([], null, "idle")).toBeNull();
	});
	test("non-idle never drains", () => {
		expect(drainCandidate([head], null, "working")).toBeNull();
		expect(drainCandidate([head], null, "approval")).toBeNull();
		expect(drainCandidate([head], null, "failed")).toBeNull();
	});
	test("editing head holds the drain", () => {
		expect(drainCandidate([head, second], "1", "idle")).toBeNull();
	});
	test("editing non-head still drains", () => {
		expect(drainCandidate([head, second], "2", "idle")).toEqual(head);
	});
});
