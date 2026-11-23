import type {
	Phase,
	PlanEntry,
	ProgressView,
	SessionKey,
	SessionRole,
} from "../../bindings";

export const PHASES: Phase[] = [
	"scoping",
	"executing",
	"queued",
	"rebasing",
	"evergreening",
	"completed",
	"cancelled",
];

export type AttentionKind = "idle" | "approval" | "failed";

export function latestRole(plan: PlanEntry): SessionRole {
	const roles = new Set(plan.sessions.map((status) => status.role));
	if (roles.has("rebasing")) {
		return "rebasing";
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
	if (plan.phase === "rebasing" && plan.queued) {
		return null;
	}
	if (plan.phase === "queued") {
		return null;
	}
	const role = latestRole(plan);
	const status = plan.sessions.find((candidate) => candidate.role === role);
	if (status === undefined) {
		return null;
	}
	if (status.failed) {
		return "failed";
	}
	if (status.approval) {
		return "approval";
	}
	if (status.working) {
		return null;
	}
	return "idle";
}

export interface HeaderProgress {
	done: number;
	total: number;
	etaSecs: number | null;
}

function toHeaderProgress(progress: ProgressView): HeaderProgress {
	return {
		done: progress.done,
		total: progress.total,
		etaSecs: progress.eta_secs ?? null,
	};
}

export function todoProgress(plan: PlanEntry): HeaderProgress | null {
	if (plan.phase !== "executing" && plan.phase !== "evergreening") {
		return null;
	}
	const role: SessionRole =
		plan.phase === "evergreening" ? "evergreening" : "executing";
	const status = plan.sessions.find((candidate) => candidate.role === role);
	const progress: ProgressView | null = status?.progress ?? null;
	if (progress === null) {
		return { done: 0, total: 0, etaSecs: null };
	}
	return toHeaderProgress(progress);
}

function rebasingProgress(plan: PlanEntry): HeaderProgress {
	const progress = plan.rebasing_progress ?? null;
	if (progress === null) {
		return { done: 0, total: 0, etaSecs: null };
	}
	return toHeaderProgress(progress);
}

export function headerProgress(plan: PlanEntry): HeaderProgress | null {
	if (plan.phase === "rebasing") {
		return rebasingProgress(plan);
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
	rebasing: "Rebasing",
};

export function roleLabel(role: SessionRole): string {
	return ROLE_LABELS[role];
}
