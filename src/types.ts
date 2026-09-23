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

export interface SessionInfo {
	session_id: string;
	repo_root: string;
	branch: string;
	config_options: ConfigOptionView[];
}

export interface RecentRepo {
	path: string;
	branch: string;
}

export interface RepoInfo {
	root: string;
	branch: string;
}

export interface Prefs {
	recent: RecentRepo[];
	last_repo: string | null;
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
	| { type: "agent_text"; chunk: string }
	| { type: "tool_line"; line: ToolLineView }
	| { type: "turn_done" }
	| { type: "turn_failed"; raw: string; hint: string; retryable: boolean }
	| { type: "agent_exited"; raw: string; hint: string; retryable: boolean }
	| { type: "permission_asked"; permission: PermissionView }
	| { type: "permission_resolved"; tool_call_id: string }
	| { type: "config_options"; options: ConfigOptionView[] }
	| { type: "todos_changed"; todos: TodoView[]; changes: TodoChangeView[] }
	| {
			type: "spend_tick";
			cost: number;
			ctx_pct: number;
	  }
	| { type: "session_reset" };

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

export type AgentStatus = "idle" | "working" | "approval";
