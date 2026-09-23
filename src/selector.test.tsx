import { render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { ModelSelector } from "./components/ModelSelector";
import type { ConfigOptionView } from "./types";

const standard: ConfigOptionView[] = [
	{
		id: "model",
		name: "Model",
		currentValue: "opencode/big-pickle",
		category: "model",
		options: [
			{ value: "opencode/big-pickle", name: "Big Pickle" },
			{ value: "opencode/muse-spark-1.3", name: "Muse Spark 1.3" },
			{
				value: "opencode/nemotron-3-ultra-with-a-very-long-name",
				name: "Nemotron 3 Ultra",
			},
		],
	},
	{
		id: "thought_level",
		name: "Thought",
		currentValue: "high",
		category: "thought_level",
		options: [
			{ value: "low", name: "low" },
			{ value: "high", name: "high" },
		],
	},
];

const customIds: ConfigOptionView[] = [
	{
		id: "llm",
		name: "LLM",
		currentValue: "a",
		category: "model",
		options: [{ value: "a", name: "A" }],
	},
	{
		id: "session_mode",
		name: "Session Mode",
		currentValue: "build",
		category: "mode",
		options: [{ value: "build", name: "build" }],
	},
	{
		id: "effort",
		name: "Effort",
		currentValue: "low",
		category: "thought_level",
		options: [{ value: "low", name: "Low" }],
	},
];

const idOnly: ConfigOptionView[] = [
	{
		id: "model",
		name: "Model",
		currentValue: "a",
		options: [{ value: "a", name: "A" }],
	},
	{
		id: "mode",
		name: "Mode",
		currentValue: "build",
		options: [{ value: "build", name: "build" }],
	},
];

describe("model selector", () => {
	test("shows current model", () => {
		render(
			<ModelSelector options={standard} disabled={false} onChange={vi.fn()} />,
		);
		expect(screen.getByText("Big Pickle")).toBeInTheDocument();
	});

	test("shows other options beside the model", () => {
		render(
			<ModelSelector options={standard} disabled={false} onChange={vi.fn()} />,
		);
		expect(screen.getByLabelText("Thought")).toBeInTheDocument();
	});

	test("disables while working", () => {
		render(
			<ModelSelector options={standard} disabled={true} onChange={vi.fn()} />,
		);
		for (const button of screen.getAllByRole("button")) {
			expect(button).toBeDisabled();
		}
	});

	test("selecting a value reports ids", async () => {
		const onChange = vi.fn();
		const user = userEvent.setup();
		render(
			<ModelSelector options={standard} disabled={false} onChange={onChange} />,
		);
		await user.click(screen.getByLabelText("Model"));
		await user.click(screen.getByText("Muse Spark 1.3"));
		expect(onChange).toHaveBeenCalledWith("model", "opencode/muse-spark-1.3");
	});

	test("finds the model option by category", () => {
		render(
			<ModelSelector options={customIds} disabled={false} onChange={vi.fn()} />,
		);
		expect(screen.getByText("A")).toBeInTheDocument();
	});

	test("hides the mode option", () => {
		render(
			<ModelSelector options={customIds} disabled={false} onChange={vi.fn()} />,
		);
		expect(screen.queryByLabelText("Session Mode")).not.toBeInTheDocument();
	});

	test("shows remaining categories as extras", () => {
		render(
			<ModelSelector options={customIds} disabled={false} onChange={vi.fn()} />,
		);
		expect(screen.getByLabelText("Effort")).toBeInTheDocument();
	});

	test("falls back to option id without a category", () => {
		render(
			<ModelSelector options={idOnly} disabled={false} onChange={vi.fn()} />,
		);
		expect(screen.getByText("A")).toBeInTheDocument();
		expect(screen.queryByLabelText("Mode")).not.toBeInTheDocument();
	});

	test("disables a dropdown with a single value", async () => {
		const user = userEvent.setup();
		render(
			<ModelSelector options={customIds} disabled={false} onChange={vi.fn()} />,
		);
		const button = screen.getByLabelText("Effort");
		expect(button).toBeDisabled();
		await user.click(button);
		expect(
			screen.queryByRole("button", { name: /Low/ }),
		).not.toBeInTheDocument();
	});

	test("shows a placeholder without an effort option", () => {
		render(
			<ModelSelector options={idOnly} disabled={false} onChange={vi.fn()} />,
		);
		const placeholder = screen.getByText("Effort unavailable");
		expect(placeholder.closest("button")).toBeDisabled();
	});

	test("sorts models alphabetically while keeping effort order", async () => {
		const user = userEvent.setup();
		const unsorted: ConfigOptionView[] = [
			{
				id: "model",
				name: "Model",
				category: "model",
				currentValue: "b",
				options: [
					{ value: "b", name: "Zulu" },
					{ value: "a", name: "alpha" },
					{ value: "c", name: "Mike" },
				],
			},
			{
				id: "effort",
				name: "Effort",
				category: "thought_level",
				currentValue: "high",
				options: [
					{ value: "low", name: "Low" },
					{ value: "high", name: "High" },
				],
			},
		];
		render(
			<ModelSelector options={unsorted} disabled={false} onChange={vi.fn()} />,
		);
		await user.click(screen.getByLabelText("Model"));
		const items = screen.getAllByRole("button", { name: /alpha|Mike|Zulu/ });
		expect(items.map((item) => item.textContent)).toEqual([
			expect.stringContaining("alpha"),
			expect.stringContaining("Mike"),
			expect.stringContaining("Zulu"),
		]);
		await user.click(screen.getByLabelText("Effort"));
		const efforts = screen.getAllByRole("button", { name: /Low|High/ });
		expect(efforts.map((item) => item.textContent)).toEqual([
			expect.stringContaining("Low"),
			expect.stringContaining("High"),
		]);
	});
});
