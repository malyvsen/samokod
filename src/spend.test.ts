import { describe, expect, test } from "vitest";
import { testEntryWith, testStatus } from "./fixtures";
import { formatContext, formatCost, planTotalCost } from "./spend";

describe("spend formatting", () => {
	test("cost always shows two decimals", () => {
		expect(formatCost(0.42)).toBe("$0.42");
		expect(formatCost(0)).toBe("$0.00");
		expect(formatCost(3.1)).toBe("$3.10");
	});

	test("context rounds to whole percent", () => {
		expect(formatContext(38.4)).toBe("38% context");
		expect(formatContext(0)).toBe("0% context");
	});
});

describe("planTotalCost", () => {
	test("sums known session costs", () => {
		const plan = testEntryWith("a", "executing", "A", false, [
			testStatus("scoping", { spend: { cost: 1.24, ctx_pct: 0 } }),
			testStatus("executing", { spend: { cost: 1.22, ctx_pct: 0 } }),
		]);
		expect(planTotalCost(plan)).toBeCloseTo(2.46);
	});

	test("skips sessions with unknown cost", () => {
		const plan = testEntryWith("a", "executing", "A", false, [
			testStatus("scoping", { spend: { cost: 1.24, ctx_pct: 0 } }),
			testStatus("executing", { spend: { cost: null, ctx_pct: null } }),
		]);
		expect(planTotalCost(plan)).toBeCloseTo(1.24);
	});

	test("returns null when every cost is unknown", () => {
		const plan = testEntryWith("a", "executing", "A", false, [
			testStatus("scoping", { spend: { cost: null, ctx_pct: null } }),
			testStatus("executing", { spend: { cost: null, ctx_pct: null } }),
		]);
		expect(planTotalCost(plan)).toBeNull();
	});
});
