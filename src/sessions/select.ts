import type { AgentStatus, PlanEntry, SessionKey } from "../types";
import { sessionKeyOf } from "../types";
import type { ChatState } from "./store";
import { emptyChat } from "./store";

export function selectedChat(
	chats: Record<string, ChatState>,
	selectedKey: SessionKey | null,
): ChatState {
	if (selectedKey === null) return emptyChat();
	return chats[sessionKeyOf(selectedKey)] ?? emptyChat();
}

export function agentStatusOf(chat: ChatState): AgentStatus {
	if (chat.approval) return "approval";
	if (chat.working) return "working";
	if (chat.failed) return "failed";
	return "idle";
}

export function selectedEntry(
	plans: PlanEntry[],
	selectedKey: SessionKey | null,
): PlanEntry | undefined {
	if (selectedKey === null) return undefined;
	return plans.find((plan) => plan.name === selectedKey.plan);
}

/// Finished plans are read-only: the selection stays on the session, but
/// no input, retry, or config change may run against it.
export function isReadOnly(entry: PlanEntry | undefined): boolean {
	return entry?.phase === "completed" || entry?.phase === "cancelled";
}

export function agentLabelForPhase(
	phase: PlanEntry["phase"] | undefined,
): string {
	switch (phase) {
		case "scoping":
			return "PLANNER";
		case "executing":
			return "EXECUTOR";
		default:
			return "AGENT";
	}
}
