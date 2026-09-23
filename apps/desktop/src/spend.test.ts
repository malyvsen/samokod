import { describe, expect, test } from "vitest";
import { formatContext, formatCost, spendLines } from "./spend";

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

	test("spend lines keep cost, context order", () => {
		expect(spendLines({ cost: 0.42, contextPct: 38.4 })).toEqual([
			"$0.42",
			"38% context",
		]);
	});
});
