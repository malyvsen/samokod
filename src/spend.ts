import type { SpendView } from "./types";

export function formatCost(cost: number): string {
	return `$${cost.toFixed(2)}`;
}

export function formatContext(contextPct: number): string {
	return `${Math.round(contextPct)}% context`;
}

export function spendLines(spend: SpendView): [string, string] {
	return [formatCost(spend.cost), formatContext(spend.contextPct)];
}
