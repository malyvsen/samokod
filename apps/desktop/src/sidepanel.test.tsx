import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SidePanel } from "./components/SidePanel";
import type { SpendView, TodoView } from "./types";

const TODOS: TodoView[] = [
	{ content: "Add retry", status: "completed", priority: "high" },
	{ content: "Wire view", status: "in_progress", priority: "high" },
	{ content: "Verify green", status: "pending", priority: "med" },
];

const SPEND_A: SpendView = {
	cost: 0.42,
	tokensIn: 148223,
	tokensOut: 2100,
	contextPct: 38.4,
};

const SPEND_B: SpendView = {
	cost: 0.45,
	tokensIn: 152223,
	tokensOut: 2300,
	contextPct: 41.2,
};

function settle() {
	act(() => {
		vi.advanceTimersByTime(10_000);
	});
}

describe("side panel", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	test("empty state shows zeros and no todos", () => {
		render(<SidePanel todos={[]} spend={null} sessionId="s1" />);
		expect(screen.getByText("$0.00")).toBeInTheDocument();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).toBeInTheDocument();
	});

	test("live list shows count and rows", () => {
		render(<SidePanel todos={TODOS} spend={SPEND_A} sessionId="s1" />);
		expect(screen.getByText("1/3")).toBeInTheDocument();
		expect(screen.getByText("Add retry")).toBeInTheDocument();
		expect(screen.getByText("$0.42")).toBeInTheDocument();
		expect(screen.getByText("148k in / 2.1k out")).toBeInTheDocument();
		expect(screen.getByText("38% context")).toBeInTheDocument();
	});

	test("spend ticks type out to the new values", () => {
		const view = render(
			<SidePanel todos={TODOS} spend={SPEND_A} sessionId="s1" />,
		);
		view.rerender(<SidePanel todos={TODOS} spend={SPEND_B} sessionId="s1" />);
		settle();
		expect(screen.getByText("$0.45")).toBeInTheDocument();
		expect(screen.getByText("152k in / 2.3k out")).toBeInTheDocument();
		expect(screen.getByText("41% context")).toBeInTheDocument();
	});

	test("session change resets instantly without typing", () => {
		const view = render(
			<SidePanel todos={TODOS} spend={SPEND_B} sessionId="s1" />,
		);
		settle();
		view.rerender(<SidePanel todos={[]} spend={null} sessionId="s2" />);
		expect(screen.getByText("$0.00")).toBeInTheDocument();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
	});
});
