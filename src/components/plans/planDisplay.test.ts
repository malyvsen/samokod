import { describe, expect, test } from "vitest";
import { testEntryWith, testStatus } from "../../fixtures";
import type { PlanEntry, PlanPhase, SessionStatusView } from "../../types";
import {
	attentionFor,
	attentionTitle,
	executingProgress,
	formatEta,
	headerKey,
	headerMeta,
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

	test("stays white when an older session is open", () => {
		const broke = plan("broke", "executing", [
			testStatus("scoping"),
			testStatus("executing", { failed: true }),
		]);
		expect(openPathFor(broke, "scoping")).toBe("sel");
	});

	test("stays white while the latest session is working", () => {
		expect(
			openPathFor(
				plan("run", "scoping", [testStatus("scoping", { working: true })]),
				"scoping",
			),
		).toBe("sel");
	});

	test("stays white for finished phases", () => {
		for (const phase of ["completed", "cancelled"] as const) {
			expect(
				openPathFor(
					plan("done", phase, [
						testStatus("scoping"),
						testStatus("executing", { failed: true }),
					]),
					"executing",
				),
			).toBe("sel");
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

describe("executingProgress", () => {
	test("returns null outside executing", () => {
		expect(
			executingProgress(plan("a", "scoping", [testStatus("scoping")])),
		).toBeNull();
		expect(
			executingProgress(
				plan("a", "landing", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("landing"),
				]),
			),
		).toBeNull();
	});

	test("missing progress counts as empty", () => {
		expect(
			executingProgress(
				plan("a", "executing", [
					testStatus("scoping"),
					testStatus("executing"),
				]),
			),
		).toEqual({ done: 0, total: 0, etaSecs: null });
	});

	test("maps the executing session progress", () => {
		const entry = plan("a", "executing", [
			testStatus("scoping"),
			testStatus("executing", {
				progress: { done: 2, total: 5, eta_secs: 480 },
			}),
		]);
		expect(executingProgress(entry)).toEqual({
			done: 2,
			total: 5,
			etaSecs: 480,
		});
	});
});

describe("formatEta", () => {
	test("floors to one minute minimum", () => {
		expect(formatEta(0)).toBe("1m");
		expect(formatEta(30)).toBe("1m");
		expect(formatEta(90)).toBe("1m");
		expect(formatEta(480)).toBe("8m");
	});

	test("shows hours above an hour", () => {
		expect(formatEta(3600)).toBe("1h");
		expect(formatEta(7200)).toBe("2h");
	});
});

describe("headerMeta", () => {
	test("empty shows no text and an empty track", () => {
		expect(headerMeta({ done: 0, total: 0, etaSecs: null }, null)).toEqual({
			text: null,
			pct: 0,
			barClass: "",
			tip: "No todos yet",
		});
	});

	test("estimating shows no text and an empty track", () => {
		expect(headerMeta({ done: 0, total: 5, etaSecs: null }, null)).toEqual({
			text: null,
			pct: 0,
			barClass: "",
			tip: "0 of 5 todos done, estimating time",
		});
	});

	test("partial shows ETA and a green bar", () => {
		expect(headerMeta({ done: 2, total: 5, etaSecs: 480 }, null)).toEqual({
			text: "8m",
			pct: 40,
			barClass: "",
			tip: "2 of 5 todos done, 8m left",
		});
	});

	test("waiting dims the bar but keeps the ETA", () => {
		for (const attention of ["idle", "approval", "failed"] as const) {
			const meta = headerMeta({ done: 2, total: 5, etaSecs: 480 }, attention);
			expect(meta.text).toBe("8m");
			expect(meta.pct).toBe(40);
			expect(meta.barClass).toBe("paused");
			expect(meta.tip).toBe("2 of 5 todos done, 8m left");
		}
	});

	test("done shows a full green bar with no text even when idle", () => {
		expect(headerMeta({ done: 5, total: 5, etaSecs: null }, "idle")).toEqual({
			text: null,
			pct: 100,
			barClass: "",
			tip: "5 of 5 todos done",
		});
	});
});
