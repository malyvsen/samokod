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

export interface ExecutingProgress {
	done: number;
	total: number;
	etaSecs: number | null;
}

export function executingProgress(plan: PlanEntry): ExecutingProgress | null {
	if (plan.phase !== "executing") {
		return null;
	}
	const status = plan.sessions.find(
		(candidate) => candidate.role === "executing",
	);
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

export function formatEta(secs: number): string {
	if (secs >= 3600) {
		return `${Math.floor(secs / 3600)}h`;
	}
	return `${Math.max(1, Math.floor(secs / 60))}m`;
}

export interface HeaderMeta {
	text: string | null;
	pct: number;
	barClass: string;
	tip: string;
}

export function headerMeta(
	progress: ExecutingProgress,
	attention: AttentionKind | null,
): HeaderMeta {
	if (progress.total === 0) {
		return {
			text: null,
			pct: 0,
			barClass: attention === null ? "" : "paused",
			tip: "No todos yet",
		};
	}
	const done = progress.done >= progress.total;
	if (done) {
		return {
			text: null,
			pct: 100,
			barClass: "",
			tip: `${progress.done} of ${progress.total} todos done`,
		};
	}
	const pct = Math.round((progress.done / progress.total) * 100);
	if (progress.etaSecs === null) {
		return {
			text: null,
			pct,
			barClass: attention === null ? "" : "paused",
			tip: `${progress.done} of ${progress.total} todos done, estimating time`,
		};
	}
	const text = formatEta(progress.etaSecs);
	return {
		text,
		pct,
		barClass: attention === null ? "" : "paused",
		tip: `${progress.done} of ${progress.total} todos done, ${text} left`,
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
	landing: "Landing",
};

export function roleLabel(role: SessionRole): string {
	return ROLE_LABELS[role];
}
