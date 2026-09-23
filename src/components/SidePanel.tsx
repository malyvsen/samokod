import { useEffect, useRef, useState } from "react";
import { spendLines } from "../spend";
import { doneCount, todoMark, todoRowClass } from "../todos";
import type { SpendView, TodoView } from "../types";

const LINE_SPEEDS = [70, 28];
const EMPTY_LINES: [string, string] = ["$0.00", "0% context"];

export function SidePanel({
	todos,
	spend,
	sessionId,
}: {
	todos: TodoView[];
	spend: SpendView | null;
	sessionId: string;
}) {
	const target = spend === null ? EMPTY_LINES : spendLines(spend);
	const [shown, typing] = useTypedLines(target, sessionId);
	const done = doneCount(todos);
	return (
		<div className="side">
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
		</div>
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
