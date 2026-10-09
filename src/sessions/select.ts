import type { AgentStatus, PlanEntry, SessionKey } from "../types";
import { sessionKeyOf } from "../types";
import type { ChatState } from "./store";

export function selectedChat(
	chats: Record<string, ChatState>,
	selectedKey: SessionKey | null,
): ChatState | null {
	if (selectedKey === null) return null;
	return chats[sessionKeyOf(selectedKey)] ?? null;
}

export function isSessionBusy(chat: ChatState | null): boolean {
	if (chat === null) return false;
	return chat.working || chat.approval || chat.start.kind === "replaying";
}

/// The pill reflects live turn state only; every start phase maps to idle.
export function agentStatusOf(chat: ChatState | null): AgentStatus {
	if (chat === null) return "idle";
	if (chat.approval) return "approval";
	if (chat.working) return "working";
	if (chat.failed) return "failed";
	return "idle";
}

/// Drain gate for one session: busy (working, approval, replaying) never
/// drains, failed never drains, otherwise idle drains.
export function drainStatusFor(
	chats: Record<string, ChatState>,
	key: SessionKey,
): AgentStatus {
	const chat = selectedChat(chats, key);
	if (isSessionBusy(chat)) return "working";
	return agentStatusOf(chat);
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
