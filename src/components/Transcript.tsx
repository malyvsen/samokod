import type { ReactNode } from "react";
import ReactMarkdown from "react-markdown";
import { todoMark, todoRowClass } from "../todos";
import type {
	PermissionOptionView,
	PermissionView,
	ToolLineView,
	TranscriptItem,
} from "../types";

export function Transcript({
	items,
	repoLabel,
	agentLabel,
	onRetry,
	onAnswer,
	children,
}: {
	items: TranscriptItem[];
	repoLabel: string;
	agentLabel: string;
	onRetry: (() => void) | null;
	onAnswer: (toolCallId: string, optionId: string) => void;
	children?: ReactNode;
}) {
	return (
		<>
			{items.length === 0 && (
				<div className="empty-hint">
					<b>{repoLabel} · fresh session</b>
					no messages yet
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
								<div className="who">{agentLabel}</div>
								<ReactMarkdown>{item.text}</ReactMarkdown>
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
									{permission.kind} · {permission.rule_hint}
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
						<div className="errbar" key={item.id}>
							<div className="erow">
								<span data-full={item.raw}>✕ {item.raw}</span>
								{item.retryable && onRetry !== null && (
									<button className="tbtn" type="button" onClick={onRetry}>
										retry
									</button>
								)}
							</div>
							<div className="ehint">{item.hint}</div>
						</div>
					);
				})}
				{children}
			</div>
		</>
	);
}

function ToolRow({ line }: { line: ToolLineView }) {
	return (
		<div className="tool" data-full={line.text}>
			{line.text}
		</div>
	);
}
