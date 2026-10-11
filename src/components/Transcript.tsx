import type { ReactNode } from "react";
import { assertNever } from "../assert";
import type { SessionStart } from "../sessions/store";
import {
	type LiveStatus,
	liveThinkingMs,
	thinkingSeconds,
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
	const status = live ?? null;
	const blocks = transcriptBlocks(items, liveThinkingMs(status));
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
				{blocks.map((block) =>
					block.kind === "thoughts" ? (
						<ThoughtRow key={block.id} ms={block.ms} />
					) : (
						<ItemRow
							key={block.item.id}
							item={block.item}
							onRetry={onRetry}
							onAnswer={onAnswer}
						/>
					),
				)}
				{status?.kind === "waiting" && (
					<div className="arow waiting">waiting</div>
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

type TranscriptBlock =
	| { kind: "thoughts"; id: string; ms: number }
	| { kind: "item"; item: Exclude<TranscriptItem, { kind: "thought" }> };

/// Adjacent thoughts read as one cumulative row. Live thinking joins a
/// trailing run, otherwise it opens its own row.
function transcriptBlocks(
	items: TranscriptItem[],
	liveMs: number | null,
): TranscriptBlock[] {
	const blocks: TranscriptBlock[] = [];
	for (const item of items) {
		if (item.kind !== "thought") {
			blocks.push({ kind: "item", item });
			continue;
		}
		const last = blocks[blocks.length - 1];
		if (last?.kind === "thoughts") {
			blocks[blocks.length - 1] = { ...last, ms: last.ms + item.ms };
		} else {
			blocks.push({ kind: "thoughts", id: item.id, ms: item.ms });
		}
	}
	if (liveMs !== null) {
		const last = blocks[blocks.length - 1];
		if (last?.kind === "thoughts") {
			blocks[blocks.length - 1] = { ...last, ms: last.ms + liveMs };
		} else {
			blocks.push({ kind: "thoughts", id: "live", ms: liveMs });
		}
	}
	return blocks;
}

function ThoughtRow({ ms }: { ms: number }) {
	return <div className="arow thought">thinking: {thinkingSeconds(ms)}s</div>;
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
			return <ToolRow line={item.line} />;
		case "todos":
			return (
				<div className="todo">
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
				</div>
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
				<div className="approval">
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
