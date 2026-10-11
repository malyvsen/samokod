import type { ReactNode } from "react";
import { assertNever } from "../assert";
import type { SessionStart } from "../sessions/store";
import {
	type LiveStatus,
	msForBurst,
	secondsForMs,
} from "../sessions/thoughts";
import { todoMark, todoRowClass } from "../todos";
import type {
	PermissionOptionView,
	PermissionView,
	ToolLineView,
	ToolStatus,
	TranscriptItem,
} from "../types";
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
	children?: ReactNode;
}) {
	const showLead =
		lead !== null &&
		!items.some((item) => item.kind === "user" && item.text === lead);
	const thinkingLive =
		live !== null && live !== undefined && live.kind === "thinking"
			? live
			: null;
	const rows: ReactNode[] = [];
	let index = 0;
	while (index < items.length) {
		const item = items[index];
		if (item === undefined) break;
		if (item.kind === "thought") {
			const firstId = item.id;
			let totalMs = 0;
			let end = index;
			while (end < items.length) {
				const next = items[end];
				if (next === undefined || next.kind !== "thought") break;
				totalMs += next.ms;
				end++;
			}
			if (end === items.length && thinkingLive !== null) {
				totalMs += msForBurst(thinkingLive.burstStart, thinkingLive.updatedAt);
			}
			rows.push(
				<div className="arow thought" key={firstId}>
					thinking: {secondsForMs(totalMs)}s
				</div>,
			);
			index = end;
			continue;
		}
		if (item.kind === "user") {
			rows.push(
				<MessageBubble
					key={item.id}
					collapseKind="user"
					tone="user"
					label="YOU"
					text={item.text}
				/>,
			);
			index++;
			continue;
		}
		if (item.kind === "agent") {
			rows.push(
				<MessageBubble
					key={item.id}
					collapseKind="agent"
					tone="agent"
					label="AI"
					text={item.text}
				/>,
			);
			index++;
			continue;
		}
		if (item.kind === "tool") {
			rows.push(<ToolRow key={item.id} line={item.line} />);
			index++;
			continue;
		}
		if (item.kind === "todos") {
			rows.push(
				<div className="todo" key={item.id}>
					<div className="head">
						<span>TODOS</span>
					</div>
					{item.changes.map((change) => (
						<div
							className={`row ${todoRowClass(change.status)}`}
							data-full={change.content}
							key={change.content}
						>
							<span className="mark">[{todoMark(change.status)}]</span>
							{change.content}
						</div>
					))}
				</div>,
			);
			index++;
			continue;
		}
		if (item.kind === "approval") {
			const permission: PermissionView = item.permission;
			const allow = permission.options.find(
				(option: PermissionOptionView) => option.kind === "allow",
			);
			const reject = permission.options.find(
				(option: PermissionOptionView) => option.kind === "reject",
			);
			rows.push(
				<div className="approval" key={item.id}>
					<h3 data-full={permission.title}>{permission.title}</h3>
					<p>
						{permission.tool} · {permission.rule_hint}
					</p>
					<p className="hint">
						Reject skips just this command - the agent keeps working.
					</p>
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
				</div>,
			);
			index++;
			continue;
		}
		rows.push(
			<ErrorBar
				key={item.id}
				raw={item.raw}
				hint={item.hint}
				onRetry={item.retryable ? onRetry : null}
			/>,
		);
		index++;
	}
	const endsWithThought =
		items.length > 0 && items[items.length - 1]?.kind === "thought";
	const showLiveThinking = thinkingLive !== null && !endsWithThought;
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
				{rows}
				{live?.kind === "waiting" && (
					<div className="arow waiting">waiting</div>
				)}
				{showLiveThinking && thinkingLive !== null && (
					<div className="arow thought">
						thinking:{" "}
						{secondsForMs(
							msForBurst(thinkingLive.burstStart, thinkingLive.updatedAt),
						)}
						s
					</div>
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

function ToolRow({ line }: { line: ToolLineView }) {
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
		<div className="tool" data-status={line.status} data-full={line.text}>
			{line.text}
		</div>
	);
}
