import { useEffect, useRef, useState } from "react";
import { spendLines } from "../spend";
import { doneCount, todoMark, todoRowClass } from "../todos";
import type { SpendView, TodoView } from "../types";
import {
	EffortPlaceholder,
	OptionDropdown,
	pendingOptions,
	type SelectorModel,
	splitOptions,
} from "./selectors";

const LINE_SPEEDS = [70, 28];
const EMPTY_LINES: [string, string] = ["$0.00", "0% context"];

export function SidePanel({
	todos,
	spend,
	sessionId,
	selectors,
	disabled,
	onChange,
}: {
	todos: TodoView[];
	spend: SpendView | null;
	sessionId: string;
	selectors: SelectorModel;
	disabled: boolean;
	onChange: (configId: string, value: string) => void;
}) {
	const target = spend === null ? EMPTY_LINES : spendLines(spend);
	const [shown, typing] = useTypedLines(target, sessionId);
	const done = doneCount(todos);
	if (selectors.kind === "pending") {
		const pending = pendingOptions(selectors.defaults);
		return (
			<div className="side">
				<Cost shown={shown} typing={typing} />
				<TodoList todos={todos} done={done} />
				<div className="pin">
					<div className="sect">
						<div className="slabel">MODEL</div>
						<OptionDropdown
							option={pending.model}
							disabled={true}
							onChange={onChange}
						/>
					</div>
					<div className="sect">
						<div className="slabel">EFFORT</div>
						<OptionDropdown
							option={pending.effort}
							disabled={true}
							onChange={onChange}
						/>
					</div>
				</div>
			</div>
		);
	}
	const { model, effort, extras } = splitOptions(selectors.options);
	const hasSelectors =
		model !== undefined || effort !== undefined || extras.length > 0;
	return (
		<div className="side">
			<Cost shown={shown} typing={typing} />
			<TodoList todos={todos} done={done} />
			{hasSelectors && (
				<div className="pin">
					{model !== undefined && (
						<div className="sect">
							<div className="slabel">MODEL</div>
							<OptionDropdown
								option={model}
								disabled={disabled}
								onChange={onChange}
							/>
						</div>
					)}
					<div className="sect">
						<div className="slabel">EFFORT</div>
						{effort !== undefined ? (
							<OptionDropdown
								option={effort}
								disabled={disabled}
								onChange={onChange}
							/>
						) : (
							<EffortPlaceholder />
						)}
					</div>
					{extras.map((option) => (
						<OptionDropdown
							key={option.id}
							option={option}
							disabled={disabled}
							onChange={onChange}
						/>
					))}
				</div>
			)}
		</div>
	);
}

function Cost({ shown, typing }: { shown: string[]; typing: number }) {
	return (
		<div className="cost">
			<b>
				{shown[0] ?? ""}
				{typing === 0 && <Caret />}
			</b>
			<span className="context">
				{shown[1] ?? ""}
				{typing === 1 && <Caret />}
			</span>
		</div>
	);
}

function TodoList({ todos, done }: { todos: TodoView[]; done: number }) {
	return (
		<>
			<div className="head">
				<span>TODOS</span>
				{todos.length > 0 && (
					<span className="count">
						{done}/{todos.length}
					</span>
				)}
			</div>
			{todos.length === 0 ? (
				<div className="row dim">
					<span className="txt">No todos yet</span>
				</div>
			) : (
				todos.map((todo) => (
					<div
						className={`row ${todoRowClass(todo.status)}`}
						data-full={todo.content}
						key={todo.content}
					>
						<span className="mark">[{todoMark(todo.status)}]</span>
						<span className="txt">{todo.content}</span>
						<span className="prio">{todo.priority}</span>
					</div>
				))
			)}
		</>
	);
}

function Caret() {
	return <span className="caret">▌</span>;
}

function reducedMotion(): boolean {
	return (
		typeof window !== "undefined" &&
		typeof window.matchMedia === "function" &&
		window.matchMedia("(prefers-reduced-motion: reduce)").matches
	);
}

function useTypedLines(
	target: string[],
	sessionId: string,
): [string[], number] {
	const [shown, setShown] = useState<string[]>(target);
	const shownRef = useRef<string[]>(shown);
	const [typing, setTyping] = useState(-1);
	const job = useRef(0);
	const lastSession = useRef(sessionId);
	const key = target.join("\n");
	// biome-ignore lint/correctness/useExhaustiveDependencies: key serializes target; listing target would restart typing every render
	useEffect(() => {
		const my = ++job.current;
		if (lastSession.current !== sessionId || reducedMotion()) {
			lastSession.current = sessionId;
			shownRef.current = target;
			setShown(target);
			setTyping(-1);
			return;
		}
		let line = 0;
		const typeLine = () => {
			if (my !== job.current) return;
			while (line < target.length && shownRef.current[line] === target[line]) {
				line += 1;
			}
			if (line >= target.length) {
				setTyping(-1);
				return;
			}
			setTyping(line);
			const text = target[line] ?? "";
			const speed = LINE_SPEEDS[Math.min(line, LINE_SPEEDS.length - 1)] ?? 28;
			let i = 0;
			const write = (value: string) => {
				const next = [...shownRef.current];
				next[line] = value;
				shownRef.current = next;
				setShown(next);
			};
			write("");
			const step = () => {
				if (my !== job.current) return;
				if (i < text.length) {
					i += 1;
					write(text.slice(0, i));
					window.setTimeout(step, speed);
				} else {
					line += 1;
					typeLine();
				}
			};
			step();
		};
		typeLine();
	}, [sessionId, key]);
	return [shown, typing];
}
