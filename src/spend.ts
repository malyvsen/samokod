export function formatCost(cost: number): string {
	return `$${cost.toFixed(2)}`;
}

export function formatContext(contextPct: number): string {
	return `${Math.round(contextPct)}% context`;
}
