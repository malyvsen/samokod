import type { TodoStatus } from "./bindings";

export function commitSubject(line: string): string {
	const index = line.indexOf(" ");
	return index < 0 ? line : line.slice(index + 1);
}

export function commitStatusFor(
	index: number,
	done: number,
	total: number,
	working: boolean,
): TodoStatus {
	if (index < done) return "completed";
	if (index === done && done < total && working) return "in_progress";
	return "pending";
}
