import { describe, expect, test } from "vitest";
import type { Phase, PlanEntry, SessionStatusView } from "../../bindings";
import { testEntryWith, testStatus } from "../../fixtures";
import {
	attentionFor,
	attentionTitle,
	barTip,
	formatEta,
	headerKey,
	headerMeta,
	headerProgress,
	latestRole,
	openPathFor,
	PHASES,
	roleLabel,
	todoProgress,
} from "./planDisplay";

function plan(
	name: string,
	phase: Phase,
	statuses: SessionStatusView[],
): PlanEntry {
	return testEntryWith(name, phase, name, true, statuses);
}

describe("latestRole", () => {
	test("prefers evergreening over rebasing over executing over scoping", () => {
		expect(
			latestRole(
				plan("a", "evergreening", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("rebasing"),
					testStatus("evergreening"),
				]),
			),
		).toBe("evergreening");
		expect(
			latestRole(
				plan("a", "rebasing", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("rebasing"),
				]),
			),
		).toBe("rebasing");
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

	test("orders queued between executing and rebasing before evergreening", () => {
		expect(PHASES).toEqual([
			"scoping",
			"executing",
			"queued",
			"rebasing",
			"evergreening",
			"completed",
			"cancelled",
		]);
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

	test("rebasing entry without a rebasing row behaves like executing", () => {
		const entry = plan("a", "rebasing", [
			testStatus("scoping"),
			testStatus("executing"),
		]);
		expect(latestRole(entry)).toBe("executing");
		expect(headerKey(entry)).toEqual({ plan: "a", role: "executing" });
		const working = plan("a", "rebasing", [
			testStatus("scoping"),
			testStatus("executing", { working: true }),
		]);
		expect(attentionFor(working)).toBeNull();
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

	test("stays quiet for plain working sessions", () => {
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

	test("approval and failure win over working", () => {
		expect(
			attentionFor(
				plan("wait", "scoping", [
					testStatus("scoping", { working: true, approval: true }),
				]),
			),
		).toBe("approval");
		expect(
			attentionFor(
				plan("broke", "executing", [
					testStatus("scoping"),
					testStatus("executing", { working: true, failed: true }),
				]),
			),
		).toBe("failed");
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

	test("suppresses idle for queued plans", () => {
		const queued = testEntryWith(
			"queued",
			"queued",
			"Queued",
			true,
			[testStatus("scoping"), testStatus("executing")],
			false,
			true,
			["abc plan work"],
		);
		expect(attentionFor(queued)).toBeNull();
		expect(todoProgress(queued)).toBeNull();
		expect(headerProgress(queued)).toBeNull();
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
		expect(roleLabel("evergreening")).toBe("Evergreening");
		expect(roleLabel("rebasing")).toBe("Rebasing");
	});
});

describe("todoProgress", () => {
	test("returns null outside executing and evergreening", () => {
		expect(
			todoProgress(plan("a", "scoping", [testStatus("scoping")])),
		).toBeNull();
		expect(
			todoProgress(
				plan("a", "rebasing", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("rebasing"),
				]),
			),
		).toBeNull();
	});

	test("missing progress counts as empty", () => {
		expect(
			todoProgress(
				plan("a", "executing", [
					testStatus("scoping"),
					testStatus("executing"),
				]),
			),
		).toEqual({ done: 0, total: 0, etaSecs: null, elapsedSecs: null });
		expect(
			todoProgress(
				plan("a", "evergreening", [
					testStatus("scoping"),
					testStatus("executing"),
					testStatus("rebasing"),
					testStatus("evergreening"),
				]),
			),
		).toEqual({ done: 0, total: 0, etaSecs: null, elapsedSecs: null });
	});

	test("maps the executing session progress", () => {
		const entry = plan("a", "executing", [
			testStatus("scoping"),
			testStatus("executing", {
				progress: { done: 2, total: 5, eta_secs: 480, elapsed_secs: 180 },
			}),
		]);
		expect(todoProgress(entry)).toEqual({
			done: 2,
			total: 5,
			etaSecs: 480,
			elapsedSecs: 180,
		});
	});

	test("maps the evergreening session progress", () => {
		const entry = plan("a", "evergreening", [
			testStatus("scoping"),
			testStatus("executing"),
			testStatus("rebasing"),
			testStatus("evergreening", {
				progress: { done: 1, total: 4, eta_secs: 120, elapsed_secs: 180 },
			}),
		]);
		expect(todoProgress(entry)).toEqual({
			done: 1,
			total: 4,
			etaSecs: 120,
			elapsedSecs: 180,
		});
	});
});

describe("headerProgress", () => {
	test("returns null outside executing, evergreening, and rebasing", () => {
		expect(
			headerProgress(plan("a", "scoping", [testStatus("scoping")])),
		).toBeNull();
		expect(
			headerProgress(plan("a", "completed", [testStatus("scoping")])),
		).toBeNull();
		expect(
			headerProgress(plan("a", "cancelled", [testStatus("scoping")])),
		).toBeNull();
	});

	test("flows executing through todo progress", () => {
		const entry = plan("a", "executing", [
			testStatus("scoping"),
			testStatus("executing", {
				progress: { done: 2, total: 5, eta_secs: 480, elapsed_secs: 180 },
			}),
		]);
		expect(headerProgress(entry)).toEqual(todoProgress(entry));
	});

	test("rebasing diverged stays empty", () => {
		const entry = testEntryWith(
			"a",
			"rebasing",
			"a",
			true,
			[testStatus("scoping"), testStatus("rebasing")],
			false,
			true,
			["abc plan work", "def more work"],
			{ done: 0, total: 2, eta_secs: null, elapsed_secs: null },
		);
		expect(headerProgress(entry)).toEqual({
			done: 0,
			total: 2,
			etaSecs: null,
			elapsedSecs: null,
		});
	});

	test("rebasing ready fills the bar", () => {
		const entry = testEntryWith(
			"a",
			"rebasing",
			"a",
			true,
			[testStatus("scoping"), testStatus("rebasing")],
			false,
			true,
			["abc plan work", "def more work"],
			{ done: 2, total: 2, eta_secs: null, elapsed_secs: null },
		);
		expect(headerProgress(entry)).toEqual({
			done: 2,
			total: 2,
			etaSecs: null,
			elapsedSecs: null,
		});
	});

	test("rebasing empty shows an empty track", () => {
		const entry = testEntryWith("a", "rebasing", "a", true, [
			testStatus("scoping"),
			testStatus("rebasing"),
		]);
		expect(headerProgress(entry)).toEqual({
			done: 0,
			total: 0,
			etaSecs: null,
			elapsedSecs: null,
		});
	});

	test("rebasing partial shows an ETA", () => {
		const entry = testEntryWith(
			"a",
			"rebasing",
			"a",
			true,
			[testStatus("scoping"), testStatus("rebasing")],
			false,
			true,
			["abc plan work", "def more work"],
			{ done: 1, total: 2, eta_secs: 600, elapsed_secs: 180 },
		);
		expect(headerProgress(entry)).toEqual({
			done: 1,
			total: 2,
			etaSecs: 600,
			elapsedSecs: 180,
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

describe("barTip", () => {
	test("shows elapsed and remaining", () => {
		expect(barTip({ done: 2, total: 5, etaSecs: 300, elapsedSecs: 180 })).toBe(
			"3m elapsed \u00b7 5m left",
		);
	});

	test("omits remaining when unknown", () => {
		expect(barTip({ done: 2, total: 5, etaSecs: null, elapsedSecs: 180 })).toBe(
			"3m elapsed",
		);
	});

	test("omits elapsed when unknown", () => {
		expect(barTip({ done: 2, total: 5, etaSecs: 300, elapsedSecs: null })).toBe(
			"5m left",
		);
	});
});

describe("headerMeta", () => {
	test("empty shows no text and an empty track", () => {
		expect(
			headerMeta({ done: 0, total: 0, etaSecs: null, elapsedSecs: null }, null),
		).toEqual({
			eta: null,
			pct: 0,
			barClass: "",
		});
	});

	test("estimating shows no text and an empty track", () => {
		expect(
			headerMeta({ done: 0, total: 5, etaSecs: null, elapsedSecs: null }, null),
		).toEqual({
			eta: null,
			pct: 0,
			barClass: "",
		});
	});

	test("partial shows ETA and a green bar", () => {
		expect(
			headerMeta({ done: 2, total: 5, etaSecs: 480, elapsedSecs: 180 }, null),
		).toEqual({
			eta: "8m",
			pct: 40,
			barClass: "",
		});
	});

	test("waiting dims the bar but keeps the ETA", () => {
		for (const attention of ["idle", "approval", "failed"] as const) {
			const meta = headerMeta(
				{ done: 2, total: 5, etaSecs: 480, elapsedSecs: 180 },
				attention,
			);
			expect(meta.eta).toBe("8m");
			expect(meta.pct).toBe(40);
			expect(meta.barClass).toBe("paused");
		}
	});

	test("done shows a full green bar with no text even when idle", () => {
		expect(
			headerMeta(
				{ done: 5, total: 5, etaSecs: null, elapsedSecs: null },
				"idle",
			),
		).toEqual({
			eta: null,
			pct: 100,
			barClass: "",
		});
	});
});
