export interface ConfigOptionValueView {
	value: string;
	name: string;
}

export interface ConfigOptionView {
	id: string;
	name: string;
	currentValue: string;
	options: ConfigOptionValueView[];
	/** Spec `category`, absent when the agent omits it. */
	category?: string | null;
}

export interface ToolLineView {
	id: string;
	text: string;
	status: string;
}

export type SessionRole = "scoping" | "executing";

export interface SessionKey {
	plan: string;
	role: SessionRole;
}

export function sessionKeyOf(key: SessionKey): string {
	return `${key.plan}::${key.role}`;
}

export function sameSession(
	left: SessionKey | null,
	right: SessionKey | null,
): boolean {
	if (left === null || right === null) return false;
	return left.plan === right.plan && left.role === right.role;
}

export interface SessionStatusView {
	role: SessionRole;
	working: boolean;
	approval: boolean;
	failed: boolean;
	live: boolean;
}

export type PlanPhase = "scoping" | "executing" | "completed" | "cancelled";

export interface PlanEntry {
	name: string;
	phase: PlanPhase;
	title: string;
	has_plan_md: boolean;
	sessions: SessionStatusView[];
}

export interface PlansUpdate {
	plans: PlanEntry[];
	selected: SessionKey;
}

export interface OpenRepoResult {
	repo_root: string;
	branch: string;
	plans: PlanEntry[];
	selected: SessionKey;
}

export interface PlanInfo {
	name: string;
	phase: PlanPhase;
	has_plan_md: boolean;
	title: string;
}

export interface RecentRepo {
	path: string;
}

export interface RepoInfo {
	root: string;
	branch: string;
}

export interface Prefs {
	recent: RecentRepo[];
}

export interface PermissionOptionView {
	id: string;
	kind: "allow" | "reject";
}

export interface PermissionView {
	tool_call_id: string;
	title: string;
	kind: string;
	options: PermissionOptionView[];
	rule_hint: string;
}

export interface TodoView {
	content: string;
	status: string;
	priority: string;
}

interface TodoChangeView {
	content: string;
	status: string;
}

export interface SpendView {
	cost: number;
	contextPct: number;
}

export type AppEvent =
	| { type: "agent_text"; session: SessionKey; chunk: string }
	| { type: "tool_line"; session: SessionKey; line: ToolLineView }
	| { type: "turn_done"; session: SessionKey }
	| {
			type: "turn_failed";
			session: SessionKey;
			raw: string;
			hint: string;
			retryable: boolean;
	  }
	| {
			type: "agent_exited";
			session: SessionKey;
			raw: string;
			hint: string;
			retryable: boolean;
	  }
	| {
			type: "permission_asked";
			session: SessionKey;
			permission: PermissionView;
	  }
	| { type: "permission_resolved"; session: SessionKey; tool_call_id: string }
	| { type: "config_options"; session: SessionKey; options: ConfigOptionView[] }
	| {
			type: "todos_changed";
			session: SessionKey;
			todos: TodoView[];
			changes: TodoChangeView[];
	  }
	| {
			type: "spend_tick";
			session: SessionKey;
			cost: number;
			ctx_pct: number;
	  }
	| { type: "plan_changed"; session: SessionKey; plan: PlanInfo }
	| { type: "branch_changed"; branch: string }
	| { type: "session_reset"; session: SessionKey }
	| { type: "plans_changed"; plans: PlanEntry[]; selected: SessionKey };

export type TranscriptItem =
	| { kind: "user"; id: string; text: string }
	| { kind: "agent"; id: string; text: string }
	| { kind: "tool"; id: string; line: ToolLineView }
	| { kind: "todos"; id: string; changes: TodoChangeView[] }
	| {
			kind: "approval";
			id: string;
			permission: PermissionView;
			resolved: boolean;
	  }
	| {
			kind: "error";
			id: string;
			raw: string;
			hint: string;
			retryable: boolean;
	  };

export type AgentStatus = "idle" | "working" | "approval" | "failed";
