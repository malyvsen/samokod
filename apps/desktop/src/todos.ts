import type { TodoView } from "./types";

export function todoMark(status: string): string {
	if (status === "completed") return "x";
	if (status === "in_progress") return ">";
	return " ";
}

export function todoRowClass(status: string): string {
	if (status === "completed") return "done";
	if (status === "in_progress") return "active";
	return "";
}

export function doneCount(todos: TodoView[]): number {
	return todos.filter((todo) => todo.status === "completed").length;
}
