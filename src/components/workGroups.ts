import type { TodoStatus } from "../bindings";
import type { MessageItem, RunItem, TranscriptItem } from "../types";

export type TranscriptGroup =
	| { kind: "message"; item: MessageItem }
	| { kind: "run"; items: [RunItem, ...RunItem[]] };

/// Maximal runs of non-message items between user and agent messages.
/// Single-item runs get a box too. Pure.
export function groupRuns(items: TranscriptItem[]): TranscriptGroup[] {
	const groups: TranscriptGroup[] = [];
	let run: RunItem[] = [];
	function flush() {
		const [first, ...rest] = run;
		if (first !== undefined) {
			groups.push({ kind: "run", items: [first, ...rest] });
			run = [];
		}
	}
	for (const item of items) {
		if (item.kind === "user" || item.kind === "agent") {
			flush();
			groups.push({ kind: "message", item });
			continue;
		}
		run.push(item);
	}
	flush();
	return groups;
}

export function todoKindLabel(status: TodoStatus): "new" | "active" | "done" {
	if (status === "pending") return "new";
	if (status === "in_progress") return "active";
	return "done";
}

type RunEntry = { label: string; count: number };

export type RunSummary = {
	entries: RunEntry[];
	counts: string;
	durationMs: number | null;
	hasFailed: boolean;
};

/// Wall-clock for a live box: `now` ends the span, `since` is the turn-ack
/// fallback for an empty placeholder with no stamped items yet.
export type LiveTiming = { now: number; since?: number | null };

/// Each box shows its own span. A frozen box spans its first to last
/// stamped item (thoughts start at `at - ms`); a live box spans its first
/// stamped item to `now`; an empty live placeholder spans ack to `now`.
/// Unstamped history has no duration. Counts sort count-desc, ties
/// alphabetical. Tool counts read the label field directly. Pure.
export function summarizeRun(items: RunItem[], live?: LiveTiming): RunSummary {
	const entries = countEntries(items);
	entries.sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
	const counts = entries
		.map(({ label, count }) => `${count}x ${label}`)
		.join(", ");
	return {
		entries,
		counts,
		durationMs: boxDurationMs(items, live),
		hasFailed: entries.some((entry) => entry.label === "failed"),
	};
}

function boxDurationMs(items: RunItem[], live?: LiveTiming): number | null {
	const starts = items.flatMap((item) => {
		const start = itemStart(item);
		return start === null ? [] : [start];
	});
	const start = starts.length > 0 ? Math.min(...starts) : (live?.since ?? null);
	if (live !== undefined) {
		if (start === null) return null;
		return Math.max(0, live.now - start);
	}
	const ends = items.flatMap((item) =>
		item.at === undefined ? [] : [item.at],
	);
	if (starts.length === 0 || ends.length === 0) return null;
	return Math.max(0, Math.max(...ends) - Math.min(...starts));
}

function itemStart(item: RunItem): number | null {
	if (item.at === undefined) return null;
	return item.kind === "thought" ? item.at - item.ms : item.at;
}

function countEntries(items: RunItem[]): RunEntry[] {
	const toolCounts = new Map<string, number>();
	let thinkCount = 0;
	const todoCounts = new Map<string, number>();
	let approvalCount = 0;
	let failedCount = 0;
	for (const item of items) {
		if (item.kind === "tool") {
			toolCounts.set(
				item.line.label,
				(toolCounts.get(item.line.label) ?? 0) + 1,
			);
			if (item.line.status === "failed") {
				failedCount += 1;
			}
		} else if (item.kind === "thought") {
			thinkCount += 1;
		} else if (item.kind === "todos") {
			for (const change of item.changes) {
				const label = `${todoKindLabel(change.status)} todo`;
				todoCounts.set(label, (todoCounts.get(label) ?? 0) + 1);
			}
		} else if (item.kind === "approval") {
			approvalCount += 1;
		} else if (item.kind === "error") {
			failedCount += 1;
		}
	}
	const entries: RunEntry[] = [];
	for (const [label, count] of toolCounts) {
		entries.push({ label, count });
	}
	if (thinkCount > 0) {
		entries.push({ label: "think", count: thinkCount });
	}
	for (const [label, count] of todoCounts) {
		entries.push({ label, count });
	}
	if (approvalCount > 0) {
		entries.push({ label: "approval", count: approvalCount });
	}
	if (failedCount > 0) {
		entries.push({ label: "failed", count: failedCount });
	}
	return entries;
}
