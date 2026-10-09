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
