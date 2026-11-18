import type { TodoView } from "../bindings";
import { commitStatusFor, commitSubject } from "../commits";
import { doneCount, todoMark, todoRowClass } from "../todos";

export function SidePanel({ todos }: { todos: TodoView[] }) {
	const done = doneCount(todos);
	return (
		<div className="side">
			<TodoList todos={todos} done={done} />
		</div>
	);
}

export function CommitsPanel({
	commits,
	done,
	total,
	working,
}: {
	commits: string[];
	done: number;
	total: number;
	working: boolean;
}) {
	return (
		<div className="side">
			<div className="head">
				<span>COMMITS</span>
				{total > 0 && (
					<span className="count">
						{done}/{total}
					</span>
				)}
			</div>
			{commits.length === 0 ? (
				<div className="row dim">
					<span className="txt">No commits yet</span>
				</div>
			) : (
				commits.map((line, index) => {
					const status = commitStatusFor(index, done, total, working);
					const subject = commitSubject(line);
					return (
						<div
							className={`row ${todoRowClass(status)}`}
							data-full={subject}
							key={line}
						>
							<span className="mark">[{todoMark(status)}]</span>
							<span className="txt">{subject}</span>
						</div>
					);
				})
			)}
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
					</div>
				))
			)}
		</>
	);
}
