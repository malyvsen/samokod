import type { PlanEntry } from "./bindings";

export function formatCost(cost: number): string {
	return `$${cost.toFixed(2)}`;
}

export function formatContext(contextPct: number): string {
	return `${Math.round(contextPct)}% context`;
}

/// Summed session cost, or null when no session reports one.
export function planTotalCost(plan: PlanEntry): number | null {
	let total = 0;
	let known = false;
	for (const status of plan.sessions) {
		const cost = status.spend.cost;
		if (cost != null) {
			total += cost;
			known = true;
		}
	}
	return known ? total : null;
}
