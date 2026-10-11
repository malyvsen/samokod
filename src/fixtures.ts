import type {
	PlanEntry,
	PlanPhase,
	ProgressView,
	RepoDefaults,
	SessionKey,
	SessionRole,
	SessionStatusView,
} from "./types";

export function testDefaults(
	overrides: Partial<RepoDefaults> = {},
): RepoDefaults {
	return { model: null, effort: null, ...overrides };
}

export function testKey(
	plan = "2026-09-25.10-54-59",
	role: SessionRole = "scoping",
): SessionKey {
	return { plan, role };
}

export function testStatus(
	role: SessionRole,
	overrides: Partial<SessionStatusView> = {},
): SessionStatusView {
	return {
		role,
		working: false,
		approval: false,
		failed: false,
		live: false,
		progress: null,
		spend: { cost: 0, ctx_pct: 0 },
		todos: [],
		...overrides,
	};
}

export function testEntry(
	name = "2026-09-25.10-54-59",
	phase: PlanPhase = "scoping",
	title = "Parallel sessions",
	has_plan_md = false,
	manual = false,
	evergreen = true,
	commits: string[] = [],
	queued = false,
	landing_progress: ProgressView | null = null,
): PlanEntry {
	const sessions =
		phase === "scoping"
			? [testStatus("scoping")]
			: phase === "evergreening"
				? [
						testStatus("scoping"),
						testStatus("executing"),
						testStatus("evergreening"),
					]
				: phase === "landing"
					? [
							testStatus("scoping"),
							testStatus("executing"),
							testStatus("evergreening"),
							testStatus("landing"),
						]
					: [testStatus("scoping"), testStatus("executing")];
	return {
		name,
		phase,
		title,
		has_plan_md,
		sessions,
		manual,
		evergreen,
		commits,
		queued,
		landing_progress,
	};
}

export function testEntryWith(
	name: string,
	phase: PlanPhase,
	title: string,
	has_plan_md: boolean,
	statuses: SessionStatusView[],
	manual = false,
	evergreen = true,
	commits: string[] = [],
	queued = false,
	landing_progress: ProgressView | null = null,
): PlanEntry {
	return {
		name,
		phase,
		title,
		has_plan_md,
		sessions: statuses,
		manual,
		evergreen,
		commits,
		queued,
		landing_progress,
	};
}
