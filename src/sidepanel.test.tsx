import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { SidePanel } from "./components/SidePanel";
import type { TodoView } from "./types";

const TODOS: TodoView[] = [
	{ content: "Add retry", status: "completed" },
	{ content: "Wire view", status: "in_progress" },
	{ content: "Verify green", status: "pending" },
];

function panel(todos: TodoView[] = []) {
	return render(<SidePanel todos={todos} />);
}

describe("side panel", () => {
	test("empty state shows no todos and no selectors", () => {
		panel();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).toBeInTheDocument();
		expect(screen.queryByText("MODEL")).not.toBeInTheDocument();
		expect(screen.queryByText("EFFORT")).not.toBeInTheDocument();
	});

	test("live list shows count and rows", () => {
		panel(TODOS);
		expect(screen.getByText("1/3")).toBeInTheDocument();
		expect(screen.getByText("Add retry")).toBeInTheDocument();
		expect(screen.getByText("Wire view")).toBeInTheDocument();
		expect(screen.getByText("Verify green")).toBeInTheDocument();
	});
});
