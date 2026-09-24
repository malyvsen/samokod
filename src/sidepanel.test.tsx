import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { SidePanel } from "./components/SidePanel";
import type { ConfigOptionView, SpendView, TodoView } from "./types";

const TODOS: TodoView[] = [
	{ content: "Add retry", status: "completed", priority: "high" },
	{ content: "Wire view", status: "in_progress", priority: "high" },
	{ content: "Verify green", status: "pending", priority: "med" },
];

const SPEND_A: SpendView = {
	cost: 0.42,
	contextPct: 38.4,
};

const SPEND_B: SpendView = {
	cost: 0.45,
	contextPct: 41.2,
};

function settle() {
	act(() => {
		vi.advanceTimersByTime(10_000);
	});
}

const OPTIONS: ConfigOptionView[] = [
	{
		id: "model",
		name: "Model",
		category: "model",
		currentValue: "b",
		options: [
			{ value: "a", name: "A" },
			{ value: "b", name: "B" },
		],
	},
	{
		id: "thought_level",
		name: "Thought",
		category: "thought_level",
		currentValue: "low",
		options: [
			{ value: "low", name: "Low" },
			{ value: "high", name: "High" },
		],
	},
];

type PanelProps = {
	todos?: TodoView[];
	spend?: SpendView | null;
	sessionId?: string;
	options?: ConfigOptionView[];
	disabled?: boolean;
	onChange?: (configId: string, value: string) => void;
};

function panelElement(props: PanelProps = {}) {
	return (
		<SidePanel
			todos={props.todos ?? []}
			spend={props.spend ?? null}
			sessionId={props.sessionId ?? "s1"}
			options={props.options ?? []}
			disabled={props.disabled ?? false}
			onChange={props.onChange ?? vi.fn()}
		/>
	);
}

function panel(props: PanelProps = {}) {
	return render(panelElement(props));
}

describe("side panel", () => {
	beforeEach(() => {
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	test("empty state shows zeros and no todos", () => {
		panel();
		expect(screen.getByText("$0.00")).toBeInTheDocument();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).toBeInTheDocument();
		expect(screen.queryByText("EFFORT")).not.toBeInTheDocument();
	});

	test("live list shows count and rows", () => {
		panel({ todos: TODOS, spend: SPEND_A });
		expect(screen.getByText("1/3")).toBeInTheDocument();
		expect(screen.getByText("Add retry")).toBeInTheDocument();
		expect(screen.getByText("$0.42")).toBeInTheDocument();
		expect(screen.getByText("38% context")).toBeInTheDocument();
	});

	test("spend ticks type out to the new values", () => {
		const view = panel({ todos: TODOS, spend: SPEND_A });
		view.rerender(panelElement({ todos: TODOS, spend: SPEND_B }));
		settle();
		expect(screen.getByText("$0.45")).toBeInTheDocument();
		expect(screen.getByText("41% context")).toBeInTheDocument();
	});

	test("session change resets instantly without typing", () => {
		const view = panel({ todos: TODOS, spend: SPEND_B });
		settle();
		view.rerender(panelElement({ sessionId: "s2" }));
		expect(screen.getByText("$0.00")).toBeInTheDocument();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
	});

	test("renders pinned model and effort selectors", () => {
		panel({ options: OPTIONS });
		expect(screen.getByText("MODEL")).toBeInTheDocument();
		expect(screen.getByText("EFFORT")).toBeInTheDocument();
		expect(screen.getByLabelText("Model")).toBeInTheDocument();
		expect(screen.getByLabelText("Thought")).toBeInTheDocument();
	});

	test("disables selectors while busy", () => {
		panel({ options: OPTIONS, disabled: true });
		expect(screen.getByLabelText("Model")).toBeDisabled();
		expect(screen.getByLabelText("Thought")).toBeDisabled();
	});

	test("reports option changes", () => {
		const onChange = vi.fn();
		panel({ options: OPTIONS, onChange });
		fireEvent.click(screen.getByLabelText("Model"));
		fireEvent.click(screen.getByText("A"));
		expect(onChange).toHaveBeenCalledWith("model", "a");
	});
});
