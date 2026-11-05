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
	| { kind: "user"; id: string; text: string; at?: number }
	| { kind: "agent"; id: string; text: string; at?: number }
	| { kind: "thought"; id: string; ms: number; text: string; at?: number }
	| { kind: "tool"; id: string; line: ToolLineView; at?: number }
	| { kind: "todos"; id: string; changes: TodoChangeView[]; at?: number }
	| {
			kind: "approval";
			id: string;
			permission: PermissionView;
			resolved: boolean;
			at?: number;
	  }
	| {
			kind: "error";
			id: string;
			raw: string;
			hint: string;
			retryable: boolean;
			at?: number;
	  };

export type AgentStatus = "idle" | "working" | "approval" | "failed";
