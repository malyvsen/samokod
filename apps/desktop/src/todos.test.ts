import { describe, expect, test } from "vitest";
import { doneCount, todoMark, todoRowClass } from "./todos";
import type { TodoView } from "./types";

function todo(status: string): TodoView {
	return { content: status, status, priority: "high" };
}

describe("todo views", () => {
	test("marks mirror agent statuses", () => {
		expect(todoMark("completed")).toBe("x");
		expect(todoMark("in_progress")).toBe(">");
		expect(todoMark("pending")).toBe(" ");
		expect(todoMark("cancelled")).toBe(" ");
	});

	test("row classes highlight done and active", () => {
		expect(todoRowClass("completed")).toBe("done");
		expect(todoRowClass("in_progress")).toBe("active");
		expect(todoRowClass("pending")).toBe("");
	});

	test("done counts completed rows only", () => {
		expect(
			doneCount([todo("completed"), todo("in_progress"), todo("pending")]),
		).toBe(1);
		expect(doneCount([])).toBe(0);
	});
});
