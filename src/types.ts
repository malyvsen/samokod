import type {
	PermissionView,
	SessionKey,
	TodoChangeView,
	ToolLineView,
} from "./bindings";

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

// True for the work roles that auto-advance. Scoping always waits for its
// manual approval, so STOP leaves it untouched.
export function isWorkRole(role: SessionKey["role"]): boolean {
	return role === "executing" || role === "evergreening" || role === "rebasing";
}

type ItemBase = {
	id: string;
	at?: number;
};

export type TranscriptItem =
	| ({ kind: "user"; text: string } & ItemBase)
	| ({ kind: "agent"; text: string } & ItemBase)
	| ({ kind: "thought"; ms: number; text: string } & ItemBase)
	| ({ kind: "tool"; line: ToolLineView } & ItemBase)
	| ({ kind: "todos"; changes: TodoChangeView[] } & ItemBase)
	| ({
			kind: "approval";
			permission: PermissionView;
			resolved: boolean;
	  } & ItemBase)
	| ({
			kind: "error";
			raw: string;
			hint: string;
			retryable: boolean;
	  } & ItemBase);

export type MessageItem = Extract<
	TranscriptItem,
	{ kind: "user" } | { kind: "agent" }
>;

export type RunItem = Exclude<
	TranscriptItem,
	{ kind: "user" } | { kind: "agent" }
>;

export type AgentStatus = "idle" | "working" | "approval" | "failed";
