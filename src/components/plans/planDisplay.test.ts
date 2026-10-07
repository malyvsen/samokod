import { describe, expect, test } from "vitest";
import { testEntryWith, testStatus } from "../../fixtures";
import type { PlanEntry, PlanPhase, SessionStatusView } from "../../types";
import {
	attentionFor,
	attentionTitle,
	headerKey,
	latestRole,
	openPathFor,
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
	test("prefers landing over executing over scoping", () => {
		expect(
			latestRole(
				plan("a", "landing", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("landing"),
				]),
			),
		).toBe("landing");
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

describe("openPathFor", () => {
	test("returns the attention kind when the latest session causes it", () => {
		expect(
			openPathFor(plan("idle", "scoping", [testStatus("scoping")]), "scoping"),
		).toBe("idle");
		expect(
			openPathFor(
				plan("wait", "scoping", [testStatus("scoping", { approval: true })]),
				"scoping",
			),
		).toBe("approval");
		expect(
			openPathFor(
				plan("broke", "executing", [
					testStatus("scoping"),
					testStatus("executing", { failed: true }),
				]),
				"executing",
			),
		).toBe("failed");
	});

	test("stays neutral when an older session is open", () => {
		const broke = plan("broke", "executing", [
			testStatus("scoping"),
			testStatus("executing", { failed: true }),
		]);
		expect(openPathFor(broke, "scoping")).toBe("neutral");
	});

	test("stays neutral while the latest session is working", () => {
		expect(
			openPathFor(
				plan("run", "scoping", [testStatus("scoping", { working: true })]),
				"scoping",
			),
		).toBe("neutral");
	});

	test("stays neutral for finished phases", () => {
		for (const phase of ["completed", "cancelled"] as const) {
			expect(
				openPathFor(
					plan("done", phase, [
						testStatus("scoping"),
						testStatus("executing", { failed: true }),
					]),
					"executing",
				),
			).toBe("neutral");
		}
	});
});

describe("roleLabel", () => {
	test("capitalizes roles", () => {
		expect(roleLabel("scoping")).toBe("Scoping");
		expect(roleLabel("executing")).toBe("Executing");
		expect(roleLabel("landing")).toBe("Landing");
	});
});
