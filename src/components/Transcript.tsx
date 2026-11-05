import { type ReactNode, useEffect, useReducer, useRef, useState } from "react";
import { assertNever } from "../assert";
import type {
	PermissionOptionView,
	PermissionView,
	ToolLineView,
	ToolStatus,
} from "../bindings";
import type { SessionStart } from "../sessions/store";
import type { LiveStatus } from "../sessions/thoughts";
import { thinkingSeconds } from "../sessions/thoughts";
import type { TranscriptItem } from "../types";
import {
	CollapseControl,
	type CollapsibleKind,
	isLongMessage,
	useCollapsed,
} from "./collapsible";
import { MessageMarkdown } from "./MessageMarkdown";

export function Transcript({
	items,
	lead = null,
	start,
	repoLabel,
	onRetry,
	onAnswer,
	onHistoryRetry = null,
	live = null,
	working = false,
	onStop = null,
	children,
}: {
	items: TranscriptItem[];
	lead?: string | null;
	start: SessionStart | null;
	repoLabel: string;
	onRetry: (() => void) | null;
	onAnswer: (toolCallId: string, optionId: string) => void;
	onHistoryRetry?: (() => void) | null;
	live?: LiveStatus | null;
	working?: boolean;
	onStop?: (() => void) | null;
	children?: ReactNode;
}) {
	const _live = live;
	void _live;
	const showLead =
		lead !== null &&
		!items.some((item) => item.kind === "user" && item.text === lead);
	const groups = groupRuns(items);
	const [, force] = useReducer((x: number) => x + 1, 0);
	useEffect(() => {
		if (!working) return;
		const id = setInterval(force, 1000);
		return () => {
			clearInterval(id);
		};
	}, [working]);
	const now = Date.now();
	return (
		<>
			<EmptyHint items={items} start={start} repoLabel={repoLabel} />
			<div className="tcol">
				{showLead && (
					<MessageBubble
						collapseKind="lead"
						tone="user"
						label="YOU"
						text={lead}
					/>
				)}
				{groups.map((group, index) =>
					group.kind === "run" ? (
						<WorkBox
							key={group.items[0]?.id ?? Math.random().toString(36)}
							items={group.items}
							onRetry={onRetry}
							onAnswer={onAnswer}
							live={working && index === groups.length - 1}
							now={now}
							onStop={onStop}
						/>
					) : (
						<ItemRow
							key={group.item.id}
							item={group.item}
							onRetry={onRetry}
							onAnswer={onAnswer}
						/>
					),
				)}
				{start?.kind === "failed" && (
					<ErrorBar
						raw={start.error.raw}
						hint={start.error.hint}
						onRetry={onHistoryRetry}
					/>
				)}
				{children}
			</div>
		</>
	);
}

export type RunItem = Exclude<
	TranscriptItem,
	{ kind: "user" } | { kind: "agent" }
>;

export type TranscriptGroup =
	| {
			kind: "message";
			item: Extract<TranscriptItem, { kind: "user" } | { kind: "agent" }>;
	  }
	| { kind: "run"; items: RunItem[] };

/// Maximal runs of non-message items between user and agent messages.
/// Single-item runs get a box too. Pure.
export function groupRuns(items: TranscriptItem[]): TranscriptGroup[] {
	const groups: TranscriptGroup[] = [];
	let run: RunItem[] = [];
	for (const item of items) {
		if (item.kind === "user" || item.kind === "agent") {
			if (run.length > 0) {
				groups.push({ kind: "run", items: run });
				run = [];
			}
			groups.push({ kind: "message", item });
			continue;
		}
		run.push(item);
	}
	if (run.length > 0) {
		groups.push({ kind: "run", items: run });
	}
	return groups;
}

export function summarizeRun(items: RunItem[]): {
	counts: string;
	durationMs: number | null;
	hasFailed: boolean;
} {
	let durationMs: number | null = null;
	const ats = items
		.map((item) => item.at)
		.filter((at): at is number => at !== undefined);
	if (ats.length === items.length && items.length > 0) {
		durationMs = Math.max(...ats) - Math.min(...ats);
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
				const label =
					change.status === "pending"
						? "new todo"
						: change.status === "in_progress"
							? "active todo"
							: "done todo";
				todoCounts.set(label, (todoCounts.get(label) ?? 0) + 1);
			}
		} else if (item.kind === "approval") {
			approvalCount += 1;
		} else if (item.kind === "error") {
			failedCount += 1;
		}
	}
	const entries: { label: string; count: number }[] = [];
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
	return { counts, durationMs, hasFailed: failedCount > 0 };
}

function WorkBox({
	items,
	onRetry,
	onAnswer,
	live = false,
	now = Date.now(),
	onStop = null,
}: {
	items: RunItem[];
	onRetry: (() => void) | null;
	onAnswer: (toolCallId: string, optionId: string) => void;
	live?: boolean;
	now?: number;
	onStop?: (() => void) | null;
}) {
	const [open, setOpen] = useState(false);
	const workRef = useRef<HTMLDivElement>(null);
	const { counts, durationMs: frozenMs, hasFailed } = summarizeRun(items);
	const ats = items
		.map((item) => item.at)
		.filter((at): at is number => at !== undefined);
	const allStamped = ats.length === items.length && items.length > 0;
	const minAt = allStamped ? Math.min(...ats) : null;
	const durationMs =
		live && minAt !== null ? now - minAt : allStamped ? frozenMs : null;
	const duration =
		durationMs === null ? null : `${(durationMs / 1000).toFixed(1)}s`;
	const verb = live ? "Working" : "Worked";
	const prefix =
		duration === null ? `${verb} · ` : `${verb} for ${duration} · `;
	const countParts = counts === "" ? [] : counts.split(", ");
	const hasUnresolvedApproval = items.some(
		(item) => item.kind === "approval" && !item.resolved,
	);
	const showReview = live && hasUnresolvedApproval;
	const showStop = live && !hasUnresolvedApproval && onStop !== null;

	function focusFirstApproval() {
		const button = workRef.current?.querySelector(
			".drow.appr button, .approval button",
		);
		if (button instanceof HTMLElement) {
			button.focus();
		}
	}

	function handleReview(e: React.MouseEvent) {
		e.stopPropagation();
		setOpen(true);
		window.setTimeout(focusFirstApproval, 0);
	}

	function handleStop(e: React.MouseEvent) {
		e.stopPropagation();
		onStop?.();
	}

	function toggle() {
		setOpen((was) => !was);
	}

	return (
		<div className={hasFailed ? "work failed" : "work"} ref={workRef}>
			<div className="summary-row">
				<button
					className="summary"
					type="button"
					aria-expanded={open}
					title={open ? "Collapse" : "Expand"}
					onClick={toggle}
				>
					{!live && <span className="chev">{open ? "▾" : "▸"}</span>}
					<span className="txt">
						{prefix}
						{countParts.map((part, index) => (
							<span key={part}>
								{index > 0 && ", "}
								{part.includes("failed") ? (
									<span style={{ color: "#ff6b6b" }}>{part}</span>
								) : (
									part
								)}
							</span>
						))}
					</span>
				</button>
				{showReview && (
					<button
						className="review-icon"
						data-review
						type="button"
						aria-label="Review the permission request"
						title="Review the permission request"
						onClick={handleReview}
					>
						?
					</button>
				)}
				{showStop && (
					<button
						className="stop-icon"
						data-stop
						type="button"
						aria-label="Stop this turn"
						title="Stop this turn"
						onClick={handleStop}
					>
						<span className="sq" aria-hidden="true" />
					</button>
				)}
			</div>
			<div className="detail" hidden={!open}>
				{items.map((item) =>
					item.kind === "thought" ? (
						<ThoughtRow
							key={item.id}
							ms={item.ms}
							text={item.text}
							{...(item.at === undefined ? {} : { at: item.at })}
						/>
					) : (
						<ItemRow
							key={item.id}
							item={item}
							onRetry={onRetry}
							onAnswer={onAnswer}
						/>
					),
				)}
			</div>
		</div>
	);
}

function formatClock(at: number): string {
	const date = new Date(at);
	const pad = (n: number) => String(n).padStart(2, "0");
	return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

function ThoughtRow({
	ms,
	text,
	at,
}: {
	ms: number;
	text: string;
	at?: number;
}) {
	const [open, setOpen] = useState(false);
	return (
		<>
			<button
				className="arow thought"
				type="button"
				title={open ? "Hide full thought" : "Show full thought"}
				onClick={() => setOpen((was) => !was)}
			>
				{at !== undefined && <span className="ts">{formatClock(at)} </span>}
				{`thinking: ${thinkingSeconds(ms)}s`}
			</button>
			<div className="thinkfull" hidden={!open}>
				{text}
			</div>
		</>
	);
}

function ItemRow({
	item,
	onRetry,
	onAnswer,
}: {
	item: Exclude<TranscriptItem, { kind: "thought" }>;
	onRetry: (() => void) | null;
	onAnswer: (toolCallId: string, optionId: string) => void;
}) {
	switch (item.kind) {
		case "user":
			return (
				<MessageBubble
					collapseKind="user"
					tone="user"
					label="YOU"
					text={item.text}
				/>
			);
		case "agent":
			return (
				<MessageBubble
					collapseKind="agent"
					tone="agent"
					label="AI"
					text={item.text}
				/>
			);
		case "tool":
			return (
				<ToolRow
					line={item.line}
					{...(item.at === undefined ? {} : { at: item.at })}
				/>
			);
		case "todos":
			return (
				<>
					{item.changes.map((change) => {
						const label =
							change.status === "pending"
								? "new"
								: change.status === "in_progress"
									? "active"
									: "done";
						return (
							<div
								className="drow"
								data-full={change.content}
								key={change.content}
							>
								{item.at !== undefined && (
									<span className="ts">{formatClock(item.at)} </span>
								)}
								{`todo ${label}: ${change.content}`}
							</div>
						);
					})}
				</>
			);
		case "approval": {
			const permission: PermissionView = item.permission;
			const allow = permission.options.find(
				(option: PermissionOptionView) => option.kind === "allow",
			);
			const reject = permission.options.find(
				(option: PermissionOptionView) => option.kind === "reject",
			);
			return (
				<div className="drow appr">
					{item.at !== undefined && (
						<span className="ts">{formatClock(item.at)} </span>
					)}
					<span data-full={permission.title}>{permission.title}</span>
					<div className="row">
						{allow !== undefined && (
							<button
								className="tbtn primary"
								type="button"
								disabled={item.resolved}
								onClick={() => onAnswer(permission.tool_call_id, allow.id)}
							>
								allow once
							</button>
						)}
						{reject !== undefined && (
							<button
								className="tbtn danger"
								type="button"
								disabled={item.resolved}
								onClick={() => onAnswer(permission.tool_call_id, reject.id)}
							>
								reject
							</button>
						)}
					</div>
				</div>
			);
		}
		case "error":
			return (
				<ErrorBar
					raw={item.raw}
					hint={item.hint}
					onRetry={item.retryable ? onRetry : null}
				/>
			);
		default:
			return assertNever(item);
	}
}

function MessageBubble({
	collapseKind,
	tone,
	label,
	text,
}: {
	collapseKind: CollapsibleKind;
	tone: "user" | "agent";
	label: string;
	text: string;
}) {
	const [collapsed, toggle] = useCollapsed(collapseKind, text);
	const long = isLongMessage(text);
	return (
		<div className={collapsed ? `msg ${tone} collapsed` : `msg ${tone}`}>
			<div className="who">
				<span>{label}</span>
				<span className="right">
					{long && <CollapseControl collapsed={collapsed} onToggle={toggle} />}
				</span>
			</div>
			<div className="body">
				<MessageMarkdown text={text} />
			</div>
		</div>
	);
}

function EmptyHint({
	items,
	start,
	repoLabel,
}: {
	items: TranscriptItem[];
	start: SessionStart | null;
	repoLabel: string;
}) {
	if (items.length > 0) return null;
	const copy = emptyHintCopy(start);
	if (copy === null) return null;
	return (
		<div className="empty-hint">
			<b>
				{repoLabel} · {copy.header}
			</b>
			{copy.sub}
		</div>
	);
}

/// Copy for the empty transcript, one row per start phase.
function emptyHintCopy(
	start: SessionStart | null,
): { header: string; sub: string } | null {
	if (start === null) {
		return { header: "opening session", sub: "getting session ready" };
	}
	switch (start.kind) {
		case "preparing":
			return { header: "fresh session", sub: "preparing session" };
		case "replaying":
			return { header: "loading history", sub: "replaying past messages" };
		case "idle":
			return { header: "fresh session", sub: "no messages yet" };
		case "failed":
			return null;
		default:
			return assertNever(start);
	}
}

function ErrorBar({
	raw,
	hint,
	onRetry,
}: {
	raw: string;
	hint: string;
	onRetry: (() => void) | null;
}) {
	return (
		<div className="errbar">
			<div className="erow">
				<span data-full={raw}>✕ {raw}</span>
				{onRetry !== null && (
					<button className="tbtn" type="button" onClick={onRetry}>
						retry
					</button>
				)}
			</div>
			<div className="ehint">{hint}</div>
		</div>
	);
}

function ToolRow({ line, at }: { line: ToolLineView; at?: number }) {
	const [open, setOpen] = useState(false);
	const status: ToolStatus = line.status;
	switch (status) {
		case "pending":
		case "in_progress":
		case "completed":
		case "failed":
			break;
		default:
			assertNever(status);
	}
	return (
		<button
			className={open ? "tool open" : "tool"}
			data-status={line.status}
			data-full={line.text}
			type="button"
			title={open ? "Collapse" : "Show full command"}
			onClick={() => setOpen((was) => !was)}
		>
			{at !== undefined && <span className="ts">{formatClock(at)} </span>}
			{line.text}
		</button>
	);
}
