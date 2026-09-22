export interface ConfigValueView {
	value: string;
	name: string;
}

export interface ConfigOptionView {
	id: string;
	name: string;
	current: string;
	options: ConfigValueView[];
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

export type AppEvent =
	| { type: "agent_text"; chunk: string }
	| { type: "tool_line"; line: ToolLineView }
	| { type: "turn_done" }
	| { type: "turn_failed"; raw: string }
	| { type: "permission_asked"; permission: PermissionView }
	| { type: "permission_resolved"; tool_call_id: string };

export type TranscriptItem =
	| { kind: "user"; id: string; text: string }
	| { kind: "agent"; id: string; text: string }
	| { kind: "tool"; id: string; line: ToolLineView }
	| {
			kind: "approval";
			id: string;
			permission: PermissionView;
			resolved: boolean;
	  }
	| { kind: "error"; id: string; raw: string };

export type AgentStatus = "idle" | "working" | "approval";
