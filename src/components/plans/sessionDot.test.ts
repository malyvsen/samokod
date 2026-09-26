import { describe, expect, test } from "vitest";
import { testStatus } from "../../fixtures";
import { dotClass, sessionLabel } from "./sessionDot";

describe("session dots", () => {
	test("working, approval, and failure flags win in order", () => {
		expect(
			dotClass("scoping", "scoping", testStatus("scoping", { working: true })),
		).toBe("running");
		expect(
			dotClass("scoping", "scoping", testStatus("scoping", { approval: true })),
		).toBe("approval");
		expect(
			dotClass(
				"scoping",
				"scoping",
				testStatus("scoping", { working: true, approval: true, failed: true }),
			),
		).toBe("failed");
	});

	test("idle sessions on active plans wait for input", () => {
		expect(dotClass("scoping", "scoping", testStatus("scoping"))).toBe("input");
		expect(dotClass("executing", "executing", testStatus("executing"))).toBe(
			"input",
		);
	});

	test("history and finished sessions stay gray", () => {
		expect(dotClass("executing", "scoping", testStatus("scoping"))).toBe(
			"done",
		);
		for (const phase of ["completed", "cancelled"] as const) {
			expect(dotClass(phase, "scoping", testStatus("scoping"))).toBe("done");
			expect(dotClass(phase, "executing", testStatus("executing"))).toBe(
				"done",
			);
		}
	});
});

describe("session labels", () => {
	test("roles read as Scoping and Execution", () => {
		expect(sessionLabel("scoping")).toBe("Scoping");
		expect(sessionLabel("executing")).toBe("Execution");
	});
});
