import { type ReactNode, useEffect, useRef } from "react";
import { assertNever } from "../assert";
import type { SessionStart } from "../sessions/store";
import type { LiveStatus } from "../sessions/thoughts";
import { todoMark, todoRowClass } from "../todos";
import type {
	PermissionOptionView,
	PermissionView,
	ToolLineView,
	ToolStatus,
	TranscriptItem,
} from "../types";
import { AgentMarkdown } from "./AgentMarkdown";

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
	return (
		<>
			<EmptyHint items={items} start={start} repoLabel={repoLabel} />
			<div className="tcol">
				{lead !== null && (
					<div className="msg user">
						<div className="who">YOU</div>
						{lead}
					</div>
				)}
				{items.map((item) => {
					if (item.kind === "user") {
						return (
							<div className="msg user" key={item.id}>
								<div className="who">YOU</div>
								{item.text}
							</div>
						);
					}
					if (item.kind === "agent") {
						return (
							<div className="msg agent" key={item.id}>
								<div className="who">AI</div>
								<AgentMarkdown text={item.text} />
							</div>
						);
					}
					if (item.kind === "tool") {
						return <ToolRow key={item.id} line={item.line} />;
					}
					if (item.kind === "todos") {
						return (
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
							</div>
						);
					}
					if (item.kind === "approval") {
						const permission: PermissionView = item.permission;
						const allow = permission.options.find(
							(option: PermissionOptionView) => option.kind === "allow",
						);
						const reject = permission.options.find(
							(option: PermissionOptionView) => option.kind === "reject",
						);
						return (
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
											onClick={() =>
												onAnswer(permission.tool_call_id, allow.id)
											}
										>
											allow once
										</button>
									)}
									{reject !== undefined && (
										<button
											className="tbtn danger"
											type="button"
											disabled={item.resolved}
											onClick={() =>
												onAnswer(permission.tool_call_id, reject.id)
											}
										>
											reject
										</button>
									)}
								</div>
							</div>
						);
					}
					if (item.kind === "thought") {
						return (
							<div className="arow thought" key={item.id}>
								thinking: {item.seconds}s
							</div>
						);
					}
					return (
						<ErrorBar
							key={item.id}
							raw={item.raw}
							hint={item.hint}
							onRetry={item.retryable ? onRetry : null}
						/>
					);
				})}
				{live?.kind === "waiting" && (
					<div className="arow waiting">waiting</div>
				)}
				{live?.kind === "thinking" && <ThinkingRow tail={live.tail} />}
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

function ThinkingRow({ tail }: { tail: string }) {
	const scrollRef = useRef<HTMLSpanElement>(null);
	useEffect(() => {
		const node = scrollRef.current;
		if (node !== null && tail.length > 0) {
			node.scrollLeft = node.scrollWidth;
		}
	}, [tail]);
	return (
		<div className="arow thinking">
			<span className="tlabel">thinking:</span>
			<span ref={scrollRef} className="tscroll" data-full={tail}>
				{tail}
			</span>
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
