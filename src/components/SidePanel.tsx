import { doneCount, todoMark, todoRowClass } from "../todos";
import type { TodoView } from "../types";

export function SidePanel({ todos }: { todos: TodoView[] }) {
	const done = doneCount(todos);
	return (
		<div className="side">
			<TodoList todos={todos} done={done} />
		</div>
	);
}

export function CommitsPanel({ commits }: { commits: string[] }) {
	return (
		<div className="side">
			<div className="head">
				<span>COMMITS</span>
				{commits.length > 0 && <span className="count">{commits.length}</span>}
			</div>
			{commits.length === 0 ? (
				<div className="row dim">
					<span className="txt">No commits yet</span>
				</div>
			) : (
				commits.map((line) => (
					<div className="row" data-full={line} key={line}>
						<span className="txt">{line}</span>
					</div>
				))
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
