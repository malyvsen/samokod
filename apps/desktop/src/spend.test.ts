import { describe, expect, test } from "vitest";
import { formatContext, formatCost, formatTokens, spendLines } from "./spend";

describe("spend formatting", () => {
	test("cost always shows two decimals", () => {
		expect(formatCost(0.42)).toBe("$0.42");
		expect(formatCost(0)).toBe("$0.00");
		expect(formatCost(3.1)).toBe("$3.10");
	});

	test("tokens use k suffix above a thousand", () => {
		expect(formatTokens(842, 12)).toBe("842 in / 12 out");
		expect(formatTokens(148223, 2100)).toBe("148k in / 2.1k out");
		expect(formatTokens(0, 0)).toBe("0 in / 0 out");
	});

	test("context rounds to whole percent", () => {
		expect(formatContext(38.4)).toBe("38% context");
		expect(formatContext(0)).toBe("0% context");
	});

	test("spend lines keep cost, tokens, context order", () => {
		expect(
			spendLines({
				cost: 0.42,
				tokensIn: 148223,
				tokensOut: 2100,
				contextPct: 38.4,
			}),
		).toEqual(["$0.42", "148k in / 2.1k out", "38% context"]);
	});
});
