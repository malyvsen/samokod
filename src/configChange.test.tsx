import { act, render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testEntry, testKey } from "./fixtures";
import type { AppEvent, ConfigOptionView } from "./types";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const api = vi.hoisted(() => ({
	getPrefs: vi.fn(),
	validateRepo: vi.fn(),
	openRepo: vi.fn(),
	createPlan: vi.fn(),
	executePlan: vi.fn(),
	markCompleted: vi.fn(),
	abandonPlan: vi.fn(),
	cancelExecution: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setConfigOption: vi.fn(),
	onAppEvent: vi.fn(() => () => {}),
}));
vi.mock("./api", () => api);

function modelOption(currentValue: string): ConfigOptionView {
	return {
		id: "model",
		name: "Model",
		category: "model",
		currentValue,
		options: [
			{ value: "openai/gpt-5", name: "GPT-5" },
			{ value: "openai/gpt-4o", name: "GPT-4o" },
		],
	};
}

function modeOption(): ConfigOptionView {
	return {
		id: "mode",
		name: "Session Mode",
		category: "mode",
		currentValue: "build",
		options: [{ value: "build", name: "build" }],
	};
}

function effortOption(currentValue: string): ConfigOptionView {
	return {
		id: "effort",
		name: "Effort",
		category: "thought_level",
		currentValue,
		options: [
			{ value: "low", name: "Low" },
			{ value: "medium", name: "Medium" },
			{ value: "high", name: "High" },
		],
	};
}

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

beforeEach(() => {
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		writable: true,
		value: () => ({
			matches: false,
			addEventListener: () => {},
			removeEventListener: () => {},
		}),
	});
	api.getPrefs.mockResolvedValue({ recent: [] });
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "main" });
	api.setConfigOption.mockReset();
});

async function openChatWith(options: ConfigOptionView[]) {
	api.getPrefs.mockResolvedValue({
		recent: [{ path: "/repo" }],
	});
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "main",
		plans: [testEntry()],
		selected: testKey(),
	});
	const user = userEvent.setup();
	render(<App />);
	await user.click(await screen.findByRole("button", { name: "open" }));
	await screen.findByRole("textbox", { name: "Ask for a change…" });
	act(() => {
		const calls = api.onAppEvent.mock.calls as unknown as Array<
			[(event: AppEvent) => void]
		>;
		calls.at(-1)?.[0]({
			type: "config_options",
			session: testKey(),
			options,
		});
	});
	return user;
}

describe("config change", () => {
	test("adopts the returned list including new options", async () => {
		const user = await openChatWith([
			modelOption("openai/gpt-4o"),
			modeOption(),
		]);
		api.setConfigOption.mockResolvedValue([
			modelOption("openai/gpt-5"),
			effortOption("low"),
			modeOption(),
		]);
		await user.click(screen.getByLabelText("Model"));
		await user.click(screen.getByText("GPT-5"));
		expect(api.setConfigOption).toHaveBeenCalledWith(
			testKey(),
			"model",
			"openai/gpt-5",
		);
		await screen.findByLabelText("Effort");
		expect(screen.getByText("GPT-5")).toBeInTheDocument();
	});

	test("ignores a stale earlier response", async () => {
		const user = await openChatWith([
			modelOption("openai/gpt-5"),
			effortOption("low"),
			modeOption(),
		]);
		const first = deferred<ConfigOptionView[]>();
		const second = deferred<ConfigOptionView[]>();
		api.setConfigOption
			.mockReturnValueOnce(first.promise)
			.mockReturnValueOnce(second.promise);
		await user.click(screen.getByLabelText("Effort"));
		await user.click(screen.getByText("High"));
		await user.click(screen.getByLabelText("Model"));
		await user.click(screen.getByText("GPT-4o"));
		first.resolve([
			modelOption("openai/gpt-5"),
			effortOption("high"),
			modeOption(),
		]);
		second.resolve([modelOption("openai/gpt-4o"), modeOption()]);
		await screen.findByText("GPT-4o");
		expect(screen.queryByLabelText("Effort")).not.toBeInTheDocument();
	});

	test("keeps the previous list when the change fails", async () => {
		const user = await openChatWith([
			modelOption("openai/gpt-5"),
			effortOption("low"),
			modeOption(),
		]);
		api.setConfigOption.mockRejectedValue(new Error("denied"));
		await user.click(screen.getByLabelText("Effort"));
		await user.click(screen.getByText("Medium"));
		await screen.findByText("Low");
		expect(screen.getByText("GPT-5")).toBeInTheDocument();
	});
});
