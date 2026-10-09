import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { SidePanel } from "./components/SidePanel";
import { toSelectorModel } from "./components/selectors";
import type { ConfigOptionView, TodoView } from "./types";

const TODOS: TodoView[] = [
	{ content: "Add retry", status: "completed" },
	{ content: "Wire view", status: "in_progress" },
	{ content: "Verify green", status: "pending" },
];

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
	options?: ConfigOptionView[];
	selectors?: import("./components/selectors").SelectorModel;
	disabled?: boolean;
	onChange?: (configId: string, value: string) => void;
};

function panelElement(props: PanelProps = {}) {
	return (
		<SidePanel
			todos={props.todos ?? []}
			selectors={
				props.selectors ??
				toSelectorModel(props.options ?? [], { model: null, effort: null })
			}
			disabled={props.disabled ?? false}
			onChange={props.onChange ?? vi.fn()}
		/>
	);
}

function panel(props: PanelProps = {}) {
	return render(panelElement(props));
}

describe("side panel", () => {
	test("empty state shows no todos and no cost", () => {
		panel();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).toBeInTheDocument();
		expect(screen.queryByText("$0.00")).not.toBeInTheDocument();
		expect(screen.queryByText(/context/)).not.toBeInTheDocument();
		expect(screen.getByText("MODEL")).toBeInTheDocument();
		expect(screen.getByText("EFFORT")).toBeInTheDocument();
		expect(screen.getByLabelText("Model")).toBeDisabled();
		expect(screen.getByLabelText("Effort")).toBeDisabled();
	});

	test("live list shows count and rows without cost", () => {
		panel({ todos: TODOS });
		expect(screen.getByText("1/3")).toBeInTheDocument();
		expect(screen.getByText("Add retry")).toBeInTheDocument();
		expect(screen.queryByText("$0.42")).not.toBeInTheDocument();
		expect(screen.queryByText(/context/)).not.toBeInTheDocument();
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

	test("pending renders disabled stored labels", () => {
		panel({
			selectors: {
				kind: "pending",
				defaults: { model: "stored-model", effort: "stored-effort" },
			},
		});
		expect(screen.getByText("MODEL")).toBeInTheDocument();
		expect(screen.getByText("EFFORT")).toBeInTheDocument();
		expect(screen.getByText("stored-model")).toBeInTheDocument();
		expect(screen.getByText("stored-effort")).toBeInTheDocument();
		expect(screen.getByLabelText("Model")).toBeDisabled();
		expect(screen.getByLabelText("Effort")).toBeDisabled();
		expect(screen.queryByText("Effort unavailable")).not.toBeInTheDocument();
	});

	test("pending without stored values shows disabled fallbacks", () => {
		panel({
			selectors: { kind: "pending", defaults: { model: null, effort: null } },
		});
		expect(screen.getByLabelText("Model")).toBeDisabled();
		expect(screen.getByLabelText("Effort")).toBeDisabled();
		expect(screen.getByText("Model")).toBeInTheDocument();
		expect(screen.getByText("Effort")).toBeInTheDocument();
	});

	test("live drops stale labels for authoritative lists", () => {
		const view = panel({
			selectors: {
				kind: "pending",
				defaults: { model: "stale-model", effort: null },
			},
		});
		expect(screen.getByText("stale-model")).toBeInTheDocument();
		view.rerender(panelElement({ options: OPTIONS }));
		expect(screen.queryByText("stale-model")).not.toBeInTheDocument();
		expect(screen.getByText("B")).toBeInTheDocument();
		expect(screen.queryByText("Effort unavailable")).not.toBeInTheDocument();
	});
});
