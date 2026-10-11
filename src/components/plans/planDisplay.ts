import type {
	PlanEntry,
	PlanPhase,
	SessionKey,
	SessionRole,
	TodoProgressView,
} from "../../types";

export const PHASES: PlanPhase[] = [
	"scoping",
	"executing",
	"evergreening",
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
	if (roles.has("evergreening")) {
		return "evergreening";
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
	if (plan.phase === "landing" && plan.queued) {
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

export interface HeaderProgress {
	done: number;
	total: number;
	etaSecs: number | null;
}

export function todoProgress(plan: PlanEntry): HeaderProgress | null {
	if (plan.phase !== "executing" && plan.phase !== "evergreening") {
		return null;
	}
	const role: SessionRole =
		plan.phase === "evergreening" ? "evergreening" : "executing";
	const status = plan.sessions.find((candidate) => candidate.role === role);
	const progress: TodoProgressView | null = status?.progress ?? null;
	if (progress === null) {
		return { done: 0, total: 0, etaSecs: null };
	}
	return {
		done: progress.done,
		total: progress.total,
		etaSecs: progress.eta_secs ?? null,
	};
}

function landingProgress(plan: PlanEntry): HeaderProgress {
	return {
		done: plan.ffable ? plan.commits.length : 0,
		total: plan.commits.length,
		etaSecs: null,
	};
}

export function headerProgress(plan: PlanEntry): HeaderProgress | null {
	if (plan.phase === "landing") {
		return landingProgress(plan);
	}
	return todoProgress(plan);
}

export function formatEta(secs: number): string {
	if (secs >= 3600) {
		return `${Math.floor(secs / 3600)}h`;
	}
	return `${Math.max(1, Math.floor(secs / 60))}m`;
}

export interface HeaderMeta {
	eta: string | null;
	pct: number;
	barClass: string;
}

export function headerMeta(
	progress: HeaderProgress,
	attention: AttentionKind | null,
): HeaderMeta {
	const paused = attention === null ? "" : "paused";
	if (progress.total === 0) {
		return {
			eta: null,
			pct: 0,
			barClass: paused,
		};
	}
	if (progress.done >= progress.total) {
		return {
			eta: null,
			pct: 100,
			barClass: "",
		};
	}
	const pct = Math.round((progress.done / progress.total) * 100);
	if (progress.etaSecs === null) {
		return {
			eta: null,
			pct,
			barClass: paused,
		};
	}
	return {
		eta: formatEta(progress.etaSecs),
		pct,
		barClass: paused,
	};
}

const ATTENTION_TITLES: Record<AttentionKind, string> = {
	idle: "needs input",
	approval: "needs approval",
	failed: "failed, needs a response",
};

export function attentionTitle(kind: AttentionKind): string {
	return ATTENTION_TITLES[kind];
}

export function openPathFor(
	plan: PlanEntry,
	role: SessionRole,
): AttentionKind | "sel" {
	const attention = attentionFor(plan);
	if (attention !== null && latestRole(plan) === role) {
		return attention;
	}
	return "sel";
}

const ROLE_LABELS: Record<SessionRole, string> = {
	scoping: "Scoping",
	executing: "Executing",
	evergreening: "Evergreening",
	landing: "Landing",
};

export function roleLabel(role: SessionRole): string {
	return ROLE_LABELS[role];
}
