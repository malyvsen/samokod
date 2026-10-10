import type { AgentStatus, PlanEntry, SessionKey, SessionRole } from "../types";
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

/// Latest role one plan owns, mirroring the header target: landing wins
/// over evergreening over executing over scoping.
function latestRoleFor(entry: PlanEntry): SessionRole {
	const roles = new Set(entry.sessions.map((status) => status.role));
	if (roles.has("landing")) return "landing";
	if (roles.has("evergreening")) return "evergreening";
	if (roles.has("executing")) return "executing";
	return "scoping";
}

function headerKeyFor(entry: PlanEntry): SessionKey {
	return { plan: entry.name, role: latestRoleFor(entry) };
}

function mostRecentKeyFor(plans: PlanEntry[]): SessionKey | null {
	const first = plans[0];
	if (first === undefined) return null;
	return headerKeyFor(first);
}

function derivesFrom(candidate: string, base: string): boolean {
	return candidate === base || candidate.startsWith(`${base}.`);
}

/// Repair the viewed selection against a pushed plan list without ever
/// stealing focus: keep the current key when its plan still exists (role
/// advances only when the entry no longer lists it), fall back to the
/// most-recent entry when it vanished, null only when the list is empty.
/// Pure.
export function repairSelection(
	plans: PlanEntry[],
	current: SessionKey | null,
): SessionKey | null {
	if (plans.length === 0) return null;
	if (current === null) return mostRecentKeyFor(plans);
	const entry = plans.find((plan) => plan.name === current.plan);
	if (entry === undefined) return mostRecentKeyFor(plans);
	if (entry.sessions.some((status) => status.role === current.role)) {
		return current;
	}
	return headerKeyFor(entry);
}

/// Follow the viewed plan's own rename through an execute/cancel response:
/// honor `selected` when it derives from the acted plan, otherwise resolve
/// the renamed entry from the fresh plan list, falling back to most-recent.
/// Pure.
export function followRename(
	plans: PlanEntry[],
	oldPlan: string,
	selected: SessionKey,
): SessionKey {
	if (derivesFrom(selected.plan, oldPlan)) return selected;
	const renamed = plans.find((entry) => derivesFrom(entry.name, oldPlan));
	if (renamed !== undefined) {
		if (renamed.sessions.some((status) => status.role === selected.role)) {
			return { plan: renamed.name, role: selected.role };
		}
		return headerKeyFor(renamed);
	}
	return mostRecentKeyFor(plans) ?? selected;
}
