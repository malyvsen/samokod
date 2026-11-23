import { type ReactNode, useEffect, useRef, useState } from "react";
import { assertNever } from "../assert";
import type {
	PermissionOptionView,
	PermissionView,
	SessionKey,
	ToolLineView,
	ToolStatus,
} from "../bindings";
import type { SessionStart } from "../sessions/store";
import { thinkingSeconds } from "../sessions/thoughts";
import type { RunItem, TranscriptItem } from "../types";
import {
	CollapseControl,
	type CollapsibleKind,
	isLongMessage,
	useCollapsed,
} from "./collapsible";
import { MessageMarkdown } from "./MessageMarkdown";
import { groupRuns, summarizeRun, todoKindLabel } from "./workGroups";

export function Transcript({
	items,
	lead = null,
	start,
	repoLabel,
	onRetry,
	onAnswer,
	onHistoryRetry = null,
	working = false,
	onStop = null,
	children,
	session,
}: {
	items: TranscriptItem[];
	lead?: string | null;
	start: SessionStart | null;
	repoLabel: string;
	onRetry: (() => void) | null;
	onAnswer: (session: SessionKey, toolCallId: string, optionId: string) => void;
	onHistoryRetry?: (() => void) | null;
	working?: boolean;
	onStop?: (() => void) | null;
	children?: ReactNode;
	session: SessionKey | null;
}) {
	const showLead =
		lead !== null &&
		!items.some((item) => item.kind === "user" && item.text === lead);
	const groups = groupRuns(items);
	const now = useNow(working);
	// Bind the owning session once so approval cards answer their own
	// session. A null session disables the card, matching onRetry/onStop.
	const boundAnswer =
		session === null
			? null
			: (toolCallId: string, optionId: string) =>
					onAnswer(session, toolCallId, optionId);
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
							key={group.items[0].id}
							items={group.items}
							onRetry={onRetry}
							onAnswer={boundAnswer}
							isLive={working && index === groups.length - 1}
							now={now}
							onStop={onStop}
						/>
					) : (
						<ItemRow
							key={group.item.id}
							item={group.item}
							onRetry={onRetry}
							onAnswer={boundAnswer}
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
	isLive = false,
	now = Date.now(),
	onStop = null,
}: {
	items: RunItem[];
	onRetry: (() => void) | null;
	onAnswer: ((toolCallId: string, optionId: string) => void) | null;
	isLive?: boolean;
	now?: number;
	onStop?: (() => void) | null;
}) {
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
	const summary = summarizeRun(items, isLive ? now : undefined);
	const duration =
		summary.durationMs === null
			? null
			: `${(summary.durationMs / 1000).toFixed(1)}s`;
	const verb = isLive ? "Working" : "Worked";
	const prefix =
		duration === null ? `${verb} · ` : `${verb} for ${duration} · `;
	const showReview = isLive && hasUnresolvedApproval;
	const showStop = isLive && onStop !== null;

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

	function handleStop() {
		onStop?.();
	}

	return (
		<div className={summary.hasFailed ? "work failed" : "work"} ref={workRef}>
			<div className="summary-row">
				<button
					className="summary"
					type="button"
					aria-expanded={open}
					title={open ? "Collapse" : "Expand"}
					onClick={() => setOpen((was) => !was)}
				>
					{!isLive && <span className="chev">{open ? "▾" : "▸"}</span>}
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
							at={item.at}
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
}: {
	item: Exclude<TranscriptItem, { kind: "thought" }>;
	onRetry: (() => void) | null;
	onAnswer: ((toolCallId: string, optionId: string) => void) | null;
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
								disabled={item.resolved || onAnswer === null}
								onClick={() => onAnswer?.(permission.tool_call_id, allow.id)}
							>
								allow once
							</button>
						)}
						{reject !== undefined && (
							<button
								className="tbtn danger"
								type="button"
								disabled={item.resolved || onAnswer === null}
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
