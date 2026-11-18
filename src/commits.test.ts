import { describe, expect, test } from "vitest";
import { commitStatusFor, commitSubject } from "./commits";

describe("commit views", () => {
	test("subjects strip the hash prefix", () => {
		expect(commitSubject("abc plan work")).toBe("plan work");
		expect(commitSubject("def more work")).toBe("more work");
		expect(commitSubject("nospace")).toBe("nospace");
	});

	test("statuses mirror todo progress over the list", () => {
		expect(commitStatusFor(0, 1, 2, true)).toBe("completed");
		expect(commitStatusFor(1, 1, 2, true)).toBe("in_progress");
		expect(commitStatusFor(1, 1, 2, false)).toBe("pending");
		expect(commitStatusFor(0, 0, 2, false)).toBe("pending");
	});

	test("done rows stay completed without a current row", () => {
		expect(commitStatusFor(0, 2, 2, false)).toBe("completed");
		expect(commitStatusFor(1, 2, 2, false)).toBe("completed");
		expect(commitStatusFor(1, 2, 2, true)).toBe("completed");
	});
});
