import type { PlanPhase, SessionRole, SessionStatusView } from "../../types";

export type DotClass = "running" | "input" | "approval" | "failed" | "done";

export function dotClass(
	phase: PlanPhase,
	role: SessionRole,
	status: SessionStatusView,
): DotClass {
	if (status.failed) return "failed";
	if (status.approval) return "approval";
	if (status.working) return "running";
	if (role === "scoping" && phase !== "scoping") return "done";
	if (role === "executing" && phase !== "executing") return "done";
	return "input";
}

export function sessionLabel(role: SessionRole): string {
	return role === "executing" ? "Execution" : "Scoping";
}
