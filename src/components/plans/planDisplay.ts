import type {
	PlanEntry,
	PlanPhase,
	SessionKey,
	SessionRole,
} from "../../types";

export const PHASES: PlanPhase[] = [
	"scoping",
	"executing",
	"landing",
	"completed",
	"cancelled",
];

export type AttentionKind = "idle" | "approval" | "failed";

export function latestRole(plan: PlanEntry): SessionRole {
	const roles = new Set(plan.sessions.map((status) => status.role));
	if (roles.has("landing")) {
		return "landing";
	}
	if (roles.has("executing")) {
		return "executing";
	}
	return "scoping";
}

export function headerKey(plan: PlanEntry): SessionKey {
	return { plan: plan.name, role: latestRole(plan) };
}

export function attentionFor(plan: PlanEntry): AttentionKind | null {
	if (plan.phase === "completed" || plan.phase === "cancelled") {
		return null;
	}
	const role = latestRole(plan);
	const status = plan.sessions.find((candidate) => candidate.role === role);
	if (status === undefined || status.working) {
		return null;
	}
	if (status.failed) {
		return "failed";
	}
	if (status.approval) {
		return "approval";
	}
	return "idle";
}

const ATTENTION_TITLES: Record<AttentionKind, string> = {
	idle: "needs input",
	approval: "needs approval",
	failed: "failed, needs a response",
};

export function attentionTitle(kind: AttentionKind): string {
	return ATTENTION_TITLES[kind];
}

const ROLE_LABELS: Record<SessionRole, string> = {
	scoping: "Scoping",
	executing: "Executing",
	landing: "Landing",
};

export function roleLabel(role: SessionRole): string {
	return ROLE_LABELS[role];
}
