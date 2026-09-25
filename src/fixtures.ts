import type {
	ConfigOptionView,
	PlanInfo,
	PlanPhase,
	SessionInfo,
} from "./types";

export function testPlan(
	phase: PlanPhase = "scoping",
	has_plan_md = false,
): PlanInfo {
	const name =
		phase === "scoping" ? "2026-09-25.10-54-59" : "2026-09-25.10-54-59.slug";
	return { name, phase, has_plan_md };
}

export function testSession(options: ConfigOptionView[]): SessionInfo {
	return {
		session_id: "ses-1",
		repo_root: "/repo",
		branch: "main",
		config_options: options,
		plan: testPlan("scoping", false),
	};
}
