import type {
	PlanEntry,
	PlanPhase,
	SessionKey,
	SessionRole,
	SessionStatusView,
} from "./types";

export function testKey(
	plan = "2026-09-25.10-54-59",
	role: SessionRole = "scoping",
): SessionKey {
	return { plan, role };
}

export function testEntry(
	name = "2026-09-25.10-54-59",
	phase: PlanPhase = "scoping",
	title = "Parallel sessions",
	has_plan_md = false,
): PlanEntry {
	const idle = (role: SessionRole): SessionStatusView => ({
		role,
		working: false,
		approval: false,
		failed: false,
		live: false,
	});
	const sessions =
		phase === "scoping"
			? [idle("scoping")]
			: [idle("scoping"), idle("executing")];
	return { name, phase, title, has_plan_md, sessions };
}
