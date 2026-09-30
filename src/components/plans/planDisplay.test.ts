import { describe, expect, test } from "vitest";
import { testEntryWith, testStatus } from "../../fixtures";
import {
	attentionFor,
	attentionTitle,
	headerKey,
	latestRole,
	roleLabel,
} from "./planDisplay";

describe("latest role", () => {
	test("pipeline order wins: merging, then executing, then scoping", () => {
		expect(
			latestRole(
				testEntryWith("a", "merging", "A", true, [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("merging"),
				]),
			),
		).toBe("merging");
		expect(
			latestRole(
				testEntryWith("a", "executing", "A", true, [
					testStatus("scoping"),
					testStatus("executing"),
				]),
			),
		).toBe("executing");
		expect(
			latestRole(
				testEntryWith("a", "scoping", "A", true, [testStatus("scoping")]),
			),
		).toBe("scoping");
	});

	test("header key selects the latest role", () => {
		expect(
			headerKey(
				testEntryWith("a", "executing", "A", true, [
					testStatus("scoping"),
					testStatus("executing"),
				]),
			),
		).toEqual({ plan: "a", role: "executing" });
	});
});

describe("attention", () => {
	test("idle, approval, and failed states map to their dot", () => {
		expect(
			attentionFor(
				testEntryWith("idle", "scoping", "Idle", true, [testStatus("scoping")]),
			),
		).toBe("idle");
		expect(
			attentionFor(
				testEntryWith("wait", "scoping", "Wait", true, [
					testStatus("scoping", { approval: true }),
				]),
			),
		).toBe("approval");
		expect(
			attentionFor(
				testEntryWith("broke", "executing", "Broke", true, [
					testStatus("scoping"),
					testStatus("executing", { failed: true }),
				]),
			),
		).toBe("failed");
	});

	test("working sessions need nothing", () => {
		expect(
			attentionFor(
				testEntryWith("run", "scoping", "Run", true, [
					testStatus("scoping", { working: true }),
				]),
			),
		).toBeNull();
		expect(
			attentionFor(
				testEntryWith("run", "executing", "Run", true, [
					testStatus("scoping"),
					testStatus("executing", { working: true }),
				]),
			),
		).toBeNull();
	});

	test("finished phases need nothing", () => {
		for (const phase of ["completed", "cancelled"] as const) {
			expect(
				attentionFor(
					testEntryWith("done", phase, "Done", true, [
						testStatus("scoping"),
						testStatus("executing"),
					]),
				),
			).toBeNull();
			expect(
				attentionFor(
					testEntryWith("broke", phase, "Broke", true, [
						testStatus("scoping"),
						testStatus("executing", { failed: true }),
					]),
				),
			).toBeNull();
		}
	});

	test("titles describe the cause", () => {
		expect(attentionTitle("idle")).toBe("needs input");
		expect(attentionTitle("approval")).toBe("needs approval");
		expect(attentionTitle("failed")).toBe("failed, needs a response");
	});
});

describe("role labels", () => {
	test("roles render capitalized", () => {
		expect(roleLabel("scoping")).toBe("Scoping");
		expect(roleLabel("executing")).toBe("Executing");
		expect(roleLabel("merging")).toBe("Merging");
	});
});
