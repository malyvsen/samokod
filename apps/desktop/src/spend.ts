import type { SpendView } from "./types";

export function formatCost(cost: number): string {
	return `$${cost.toFixed(2)}`;
}

export function formatTokens(tokensIn: number, tokensOut: number): string {
	return `${thousands(tokensIn)} in / ${thousands(tokensOut)} out`;
}

export function formatContext(contextPct: number): string {
	return `${Math.round(contextPct)}% context`;
}

export function spendLines(spend: SpendView): [string, string, string] {
	return [
		formatCost(spend.cost),
		formatTokens(spend.tokensIn, spend.tokensOut),
		formatContext(spend.contextPct),
	];
}

function thousands(value: number): string {
	if (value < 1000) return `${value}`;
	const k = value / 1000;
	const rounded = k >= 100 ? Math.round(k) : Math.round(k * 10) / 10;
	return `${rounded}k`;
}
