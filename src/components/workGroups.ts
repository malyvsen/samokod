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

/// Counts sort count-desc, ties alphabetical. Tool counts read the label
/// field directly. Start is the minimum of all available evidence (stamped
/// item `at` values, thought inferred starts `at - ms`, plus passed start
/// anchors); end is `now` for a live box, otherwise the maximum stamped
/// `at`. With no start or no end (e.g. unstamped replayed history) the
/// duration stays null.
export function summarizeRun(
	items: RunItem[],
	now?: number,
	starts: number[] = [],
): RunSummary {
	let durationMs: number | null = null;
	const atValues: number[] = [...starts];
	for (const item of items) {
		if (item.at !== undefined) {
			atValues.push(item.at);
			if (item.kind === "thought") {
				atValues.push(item.at - item.ms);
			}
		}
	}
	const start = atValues.length > 0 ? Math.min(...atValues) : null;
	const end =
		now !== undefined
			? now
			: items.some((item) => item.at !== undefined)
				? Math.max(
						...items.flatMap((item) =>
							item.at !== undefined ? [item.at] : [],
						),
					)
				: null;
	if (start !== null && end !== null) {
		durationMs = end - start;
	}
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
	entries.sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
	const counts = entries
		.map(({ label, count }) => `${count}x ${label}`)
		.join(", ");
	return { entries, counts, durationMs, hasFailed: failedCount > 0 };
}
