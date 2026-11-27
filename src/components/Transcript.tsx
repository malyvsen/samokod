import { type ReactNode, useEffect, useRef, useState } from "react";
import { assertNever } from "../assert";
import type {
	PermissionOptionView,
	PermissionView,
	Phase,
	SessionKey,
	ToolLineView,
	ToolStatus,
} from "../bindings";
import type { LiveTurn, SessionStart } from "../sessions/store";
import { thinkingSeconds } from "../sessions/thoughts";
import type { RunItem, TranscriptItem } from "../types";
import {
	CollapseControl,
	type CollapsibleKind,
	isLongMessage,
	useCollapsed,
} from "./collapsible";
import { MessageMarkdown } from "./MessageMarkdown";
import {
	groupRuns,
	type LiveTiming,
	type RunSummary,
	summarizeRun,
	type TranscriptGroup,
	todoKindLabel,
} from "./workGroups";

export function Transcript({
	items,
	start,
	repoLabel,
	onRetry,
	onAnswer,
	onHistoryRetry = null,
	turn = null,
	onStop = null,
	children,
	session,
	phase = null,
}: {
	items: TranscriptItem[];
	start: SessionStart | null;
	repoLabel: string;
	onRetry: (() => void) | null;
	onAnswer: (session: SessionKey, toolCallId: string, optionId: string) => void;
	onHistoryRetry?: (() => void) | null;
	turn?: LiveTurn | null;
	onStop?: (() => void) | null;
	children?: ReactNode;
	session: SessionKey | null;
	phase?: Phase | null;
}) {
	const now = useNow(turn !== null);
	const displayItems = withPendingThought(items, turn, now);
	const groups = groupRuns(displayItems);
	// Only the trailing run hosts the live box; a trailing message drops the
	// timing (an agent message needs no box, a user message gets the empty
	// placeholder below). Non-empty boxes measure from their own items, so
	// `since` is only the empty-placeholder fallback.
	const liveIndex = turn === null ? null : groups.length - 1;
	const timing: LiveTiming | null =
		turn === null ? null : { now, since: turn.since };
	// Bind the owning session once so approval cards answer their own
	// session. A null session disables the card, matching onRetry/onStop.
	const boundAnswer =
		session === null
			? null
			: (toolCallId: string, optionId: string) =>
					onAnswer(session, toolCallId, optionId);
	// Run positions are stable because a turn only appends, so position
	// keys keep box and row expansion intact across streaming chunks and
	// live-freeze transitions. The session key resets expansion when the
	// viewed session changes.
	const sessionKey =
		session === null ? "null" : `${session.plan}::${session.role}`;
	return (
		<>
			<EmptyHint items={items} start={start} repoLabel={repoLabel} />
			<div className="tcol">
				{groups.map((group, index) =>
					group.kind === "run" ? (
						<WorkBox
							// biome-ignore lint/suspicious/noArrayIndexKey: append-only runs keep positions stable
							key={`run-${sessionKey}-${index}`}
							items={group.items}
							onRetry={onRetry}
							onAnswer={boundAnswer}
							live={index === liveIndex ? timing : null}
							onStop={onStop}
							phase={phase}
						/>
					) : (
						<ItemRow
							key={group.item.id}
							item={group.item}
							onRetry={onRetry}
							onAnswer={boundAnswer}
							phase={phase}
						/>
					),
				)}
				{needsPlaceholder(groups, turn) && (
					<EmptyWorkBox
						key="__live-placeholder__"
						live={timing}
						onStop={onStop}
					/>
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

/// The pending thought as a synthetic trailing item, so the existing run
/// grouping produces the live box with no special branches. Never persisted:
/// the next content event freezes the real thought.
function withPendingThought(
	items: TranscriptItem[],
	turn: LiveTurn | null,
	now: number,
): TranscriptItem[] {
	if (turn?.live == null) return items;
	const live = turn.live;
	return [
		...items,
		{
			kind: "thought",
			id: "__live__",
			ms: Math.max(0, now - live.startedAt),
			text: live.text,
			at: now,
		},
	];
}

/// An empty live box is needed when the turn is acked but the last group
/// can't host it: an empty transcript or one ending in a user message.
/// (An agent message needs no box; a trailing run hosts its own.)
function needsPlaceholder(
	groups: TranscriptGroup[],
	turn: LiveTurn | null,
): boolean {
	if (turn === null) return false;
	const last = groups[groups.length - 1];
	if (last === undefined) return true;
	return last.kind === "message" && last.item.kind === "user";
}

/// Wall-clock for the live work box, ticking once per second while active.
function useNow(active: boolean): number {
	const [now, setNow] = useState(() => Date.now());
	useEffect(() => {
		if (!active) return;
		setNow(Date.now());
		const id = setInterval(() => setNow(Date.now()), 1000);
		return () => {
			clearInterval(id);
		};
	}, [active]);
	return now;
}

function WorkBox({
	items,
	onRetry,
	onAnswer,
	live = null,
	onStop = null,
	phase,
}: {
	items: [RunItem, ...RunItem[]];
	onRetry: (() => void) | null;
	onAnswer: ((toolCallId: string, optionId: string) => void) | null;
	live?: LiveTiming | null;
	onStop?: (() => void) | null;
	phase: Phase | null;
}) {
	const isLive = live !== null;
	const hasUnresolvedApproval = items.some(
		(item) => item.kind === "approval" && !item.resolved,
	);
	const [open, setOpen] = useState(hasUnresolvedApproval);
	useEffect(() => {
		if (hasUnresolvedApproval) {
			setOpen(true);
		}
	}, [hasUnresolvedApproval]);
	const workRef = useRef<HTMLDivElement>(null);
	const summary = summarizeRun(items, live ?? undefined);

	function toggle() {
		setOpen((was) => !was);
	}

	function focusFirstApproval() {
		const button = workRef.current?.querySelector(".drow.appr button");
		if (button instanceof HTMLElement) {
			button.focus();
		}
	}

	function handleReview() {
		setOpen(true);
		window.setTimeout(focusFirstApproval, 0);
	}

	return (
		<div className={summary.hasFailed ? "work failed" : "work"} ref={workRef}>
			<div className="summary-row">
				<button
					className="summary"
					type="button"
					aria-expanded={open}
					title={open ? "Collapse" : "Expand"}
					onClick={toggle}
				>
					<WorkSummaryText summary={summary} live={live} />
				</button>
				{isLive && hasUnresolvedApproval && (
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
				{isLive && onStop !== null && <WorkStopButton onStop={onStop} />}
				<span className="right">
					<CollapseControl collapsed={!open} onToggle={toggle} />
				</span>
			</div>
			<div className="detail" hidden={!open}>
				{items.map((item, rowIndex) => (
					<RunRow
						// biome-ignore lint/suspicious/noArrayIndexKey: append-only rows keep positions stable
						key={rowIndex}
						item={item}
						onRetry={onRetry}
						onAnswer={onAnswer}
						phase={phase}
					/>
				))}
			</div>
		</div>
	);
}

/// The acked-but-empty live box: no run items yet, so nothing to expand.
/// Always collapsed with a disabled control; the transcript scrolls once
/// content lands and this is replaced by a real run box.
function EmptyWorkBox({
	live,
	onStop,
}: {
	live: LiveTiming | null;
	onStop: (() => void) | null;
}) {
	const summary = summarizeRun([], live ?? undefined);
	return (
		<div className="work">
			<div className="summary-row">
				<button
					className="summary"
					type="button"
					aria-expanded={false}
					aria-disabled={true}
					title="Expand"
				>
					<WorkSummaryText summary={summary} live={live} />
				</button>
				{onStop !== null && <WorkStopButton onStop={onStop} />}
				<span className="right">
					<CollapseControl collapsed disabled />
				</span>
			</div>
			<div className="detail" hidden />
		</div>
	);
}

function WorkSummaryText({
	summary,
	live,
}: {
	summary: RunSummary;
	live: LiveTiming | null;
}) {
	const duration =
		summary.durationMs === null
			? null
			: `${Math.floor(summary.durationMs / 1000)}s`;
	const verb = live !== null ? "Working" : "Worked";
	const base = duration === null ? verb : `${verb} for ${duration}`;
	const prefix = summary.entries.length === 0 ? base : `${base} · `;
	return (
		<span className="txt">
			{prefix}
			{summary.entries.map((entry, index) => (
				<span key={entry.label}>
					{index > 0 && ", "}
					{entry.label === "failed" ? (
						<span className="failed">{`${entry.count}x ${entry.label}`}</span>
					) : (
						`${entry.count}x ${entry.label}`
					)}
				</span>
			))}
		</span>
	);
}

function WorkStopButton({ onStop }: { onStop: () => void }) {
	return (
		<button
			className="stop-icon"
			data-stop
			type="button"
			aria-label="Stop this turn"
			title="Stop this turn"
			onClick={onStop}
		>
			<span className="sq" aria-hidden="true" />
		</button>
	);
}

/// One run row, keyed by position so streaming growth never remounts
/// open rows. Delegates so the position key is written exactly once.
function RunRow({
	item,
	onRetry,
	onAnswer,
	phase,
}: {
	item: RunItem;
	onRetry: (() => void) | null;
	onAnswer: ((toolCallId: string, optionId: string) => void) | null;
	phase: Phase | null;
}) {
	if (item.kind === "thought") {
		return <ThoughtRow ms={item.ms} text={item.text} at={item.at} />;
	}
	return (
		<ItemRow item={item} onRetry={onRetry} onAnswer={onAnswer} phase={phase} />
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
	at: number | undefined;
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
	phase,
}: {
	item: Exclude<TranscriptItem, { kind: "thought" }>;
	onRetry: (() => void) | null;
	onAnswer: ((toolCallId: string, optionId: string) => void) | null;
	phase: Phase | null;
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
			return <ToolRow line={item.line} at={item.at} />;
		case "todos":
			return (
				<>
					{item.changes.map((change) => (
						<div
							className="drow"
							data-full={change.content}
							key={change.content}
						>
							{item.at !== undefined && (
								<span className="ts">{formatClock(item.at)} </span>
							)}
							{`todo ${todoKindLabel(change.status)}: ${change.content}`}
						</div>
					))}
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
			const readOnly = phase === "completed" || phase === "cancelled";
			const readOnlyTitle =
				readOnly && phase !== null ? `Plan is ${phase}` : undefined;
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
								disabled={item.resolved || readOnly || onAnswer === null}
								title={readOnlyTitle}
								onClick={() => onAnswer?.(permission.tool_call_id, allow.id)}
							>
								allow once
							</button>
						)}
						{reject !== undefined && (
							<button
								className="tbtn danger"
								type="button"
								disabled={item.resolved || readOnly || onAnswer === null}
								title={readOnlyTitle}
								onClick={() => onAnswer?.(permission.tool_call_id, reject.id)}
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

/// Copy for the empty transcript, one row per start phase. The unknown
/// entry flickers only before the preparing event, so it shares the
/// preparing copy for one steady line.
function emptyHintCopy(
	start: SessionStart | null,
): { header: string; sub: string } | null {
	if (start === null || start.kind === "preparing") {
		return { header: "fresh session", sub: "preparing session" };
	}
	switch (start.kind) {
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

function ToolRow({ line, at }: { line: ToolLineView; at: number | undefined }) {
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
