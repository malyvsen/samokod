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
	return role === "executing" || role === "evergreening" || role === "landing";
}

export type TranscriptItem =
	| { kind: "user"; id: string; text: string }
	| { kind: "agent"; id: string; text: string }
	| { kind: "thought"; id: string; ms: number }
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
