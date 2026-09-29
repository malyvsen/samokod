import type { ReactNode } from "react";
import { todoMark, todoRowClass } from "../todos";
import type {
	PermissionOptionView,
	PermissionView,
	ToolKindLabel,
	ToolLineView,
	ToolStatus,
	TranscriptItem,
} from "../types";
import { AgentMarkdown } from "./AgentMarkdown";

export function Transcript({
	items,
	repoLabel,
	onRetry,
	onAnswer,
	historyLoading = false,
	historyError = null,
	onHistoryRetry = null,
	children,
}: {
	items: TranscriptItem[];
	repoLabel: string;
	onRetry: (() => void) | null;
	onAnswer: (toolCallId: string, optionId: string) => void;
	historyLoading?: boolean;
	historyError?: string | null;
	onHistoryRetry?: (() => void) | null;
	children?: ReactNode;
}) {
	return (
		<>
			{items.length === 0 && historyError === null && (
				<div className="empty-hint">
					<b>
						{repoLabel} · {historyLoading ? "loading history" : "fresh session"}
					</b>
					{historyLoading ? "replaying past messages" : "no messages yet"}
				</div>
			)}
			<div className="tcol">
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
						const kind: ToolKindLabel = permission.kind;
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
									{kind} · {permission.rule_hint}
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
					return (
						<ErrorBar
							key={item.id}
							raw={item.raw}
							hint={item.hint}
							onRetry={item.retryable ? onRetry : null}
						/>
					);
				})}
				{historyError !== null && (
					<ErrorBar
						raw={historyError}
						hint="history failed to load"
						onRetry={onHistoryRetry}
					/>
				)}
				{children}
			</div>
		</>
	);
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

function assertNever(value: never): never {
	throw new Error(`unexpected value: ${String(value)}`);
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
