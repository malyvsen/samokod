import type {
	PlanEntry,
	PlanPhase,
	RepoDefaults,
	SessionKey,
	SessionRole,
	SessionStatusView,
	WorktreeStatus,
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
		...overrides,
	};
}

export function testEntry(
	name = "2026-09-25.10-54-59",
	phase: PlanPhase = "scoping",
	title = "Parallel sessions",
	has_plan_md = false,
): PlanEntry {
	const sessions =
		phase === "scoping"
			? [testStatus("scoping")]
			: [testStatus("scoping"), testStatus("executing")];
	return { name, phase, title, has_plan_md, sessions, worktree: null };
}

export function testEntryWith(
	name: string,
	phase: PlanPhase,
	title: string,
	has_plan_md: boolean,
	statuses: SessionStatusView[],
	worktree: WorktreeStatus | null = null,
): PlanEntry {
	return { name, phase, title, has_plan_md, sessions: statuses, worktree };
}

export function testWorktree(
	overrides: Partial<WorktreeStatus> = {},
): WorktreeStatus {
	return { branch: "samokod/shiny", dirty: false, ffable: true, ...overrides };
}
