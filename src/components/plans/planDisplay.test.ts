import { describe, expect, test } from "vitest";
import { testEntryWith, testStatus } from "../../fixtures";
import type { PlanEntry, PlanPhase, SessionStatusView } from "../../types";
import {
	attentionFor,
	attentionTitle,
	headerKey,
	latestRole,
	roleLabel,
} from "./planDisplay";

function plan(
	name: string,
	phase: PlanPhase,
	statuses: SessionStatusView[],
): PlanEntry {
	return testEntryWith(name, phase, name, true, statuses);
}

describe("latestRole", () => {
	test("prefers merging over executing over scoping", () => {
		expect(
			latestRole(
				plan("a", "merging", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("merging"),
				]),
			),
		).toBe("merging");
		expect(
			latestRole(
				plan("a", "executing", [
					testStatus("scoping"),
					testStatus("executing"),
				]),
			),
		).toBe("executing");
		expect(latestRole(plan("a", "scoping", [testStatus("scoping")]))).toBe(
			"scoping",
		);
	});

	test("falls back to scoping without sessions", () => {
		expect(latestRole(plan("empty", "scoping", []))).toBe("scoping");
	});
});

describe("headerKey", () => {
	test("targets the latest role", () => {
		expect(
			headerKey(
				plan("a", "executing", [
					testStatus("scoping"),
					testStatus("executing"),
				]),
			),
		).toEqual({ plan: "a", role: "executing" });
	});

	test("falls back to scoping without sessions", () => {
		expect(headerKey(plan("empty", "scoping", []))).toEqual({
			plan: "empty",
			role: "scoping",
		});
	});
});

describe("attentionFor", () => {
	test("flags plans waiting on the user", () => {
		expect(attentionFor(plan("idle", "scoping", [testStatus("scoping")]))).toBe(
			"idle",
		);
		expect(
			attentionFor(
				plan("wait", "scoping", [testStatus("scoping", { approval: true })]),
			),
		).toBe("approval");
		expect(
			attentionFor(
				plan("broke", "executing", [
					testStatus("scoping"),
					testStatus("executing", { failed: true }),
				]),
			),
		).toBe("failed");
	});

	test("ignores working sessions", () => {
		expect(
			attentionFor(
				plan("run", "scoping", [testStatus("scoping", { working: true })]),
			),
		).toBeNull();
		expect(
			attentionFor(
				plan("run", "executing", [
					testStatus("scoping"),
					testStatus("executing", { working: true }),
				]),
			),
		).toBeNull();
	});

	test("ignores finished plans", () => {
		for (const phase of ["completed", "cancelled"] as const) {
			expect(
				attentionFor(
					plan("done", phase, [testStatus("scoping"), testStatus("executing")]),
				),
			).toBeNull();
			expect(
				attentionFor(
					plan("broke", phase, [
						testStatus("scoping"),
						testStatus("executing", { failed: true }),
					]),
				),
			).toBeNull();
		}
	});

	test("ignores plans without sessions", () => {
		expect(attentionFor(plan("empty", "scoping", []))).toBeNull();
	});
});

describe("attentionTitle", () => {
	test("names the cause", () => {
		expect(attentionTitle("idle")).toBe("needs input");
		expect(attentionTitle("approval")).toBe("needs approval");
		expect(attentionTitle("failed")).toBe("failed, needs a response");
	});
});

describe("roleLabel", () => {
	test("capitalizes roles", () => {
		expect(roleLabel("scoping")).toBe("Scoping");
		expect(roleLabel("executing")).toBe("Executing");
		expect(roleLabel("merging")).toBe("Merging");
	});
});
