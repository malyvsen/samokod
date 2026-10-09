import { doneCount, todoMark, todoRowClass } from "../todos";
import type { TodoView } from "../types";
import {
	EffortPlaceholder,
	OptionDropdown,
	pendingOptions,
	type SelectorModel,
	splitOptions,
} from "./selectors";

export function SidePanel({
	todos,
	selectors,
	disabled,
	onChange,
}: {
	todos: TodoView[];
	selectors: SelectorModel;
	disabled: boolean;
	onChange: (configId: string, value: string) => void;
}) {
	const done = doneCount(todos);
	if (selectors.kind === "pending") {
		const pending = pendingOptions(selectors.defaults);
		return (
			<div className="side">
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
