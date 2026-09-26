import { act, render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testDefaults, testEntry, testKey } from "./fixtures";
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
	selectPlan: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setConfigOption: vi.fn(),
	warmSession: vi.fn(),
	scopingDraft: vi.fn(),
	onAppEvent: vi.fn(() => () => {}),
}));
vi.mock("./api", () => api);

function authoritative(): ConfigOptionView[] {
	return [
		{
			id: "model",
			name: "Model",
			category: "model",
			currentValue: "real-model",
			options: [
				{ value: "real-model", name: "Real Model" },
				{ value: "other-model", name: "Other Model" },
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
}

function emit(event: AppEvent) {
	const calls = api.onAppEvent.mock.calls as unknown as Array<
		[(event: AppEvent) => void]
	>;
	const call = calls.at(-1);
	if (call === undefined) throw new Error("no app event subscription");
	act(() => {
		call[0](event);
	});
}

beforeEach(() => {
	vi.clearAllMocks();
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		writable: true,
		value: () => ({
			matches: false,
			addEventListener: () => {},
			removeEventListener: () => {},
		}),
	});
	api.getPrefs.mockResolvedValue({ recent: [{ path: "/repo" }] });
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "main" });
	api.warmSession.mockResolvedValue(undefined);
	api.scopingDraft.mockResolvedValue(null);
});

describe("pending pickers", () => {
	test("createPlan with empty options warms and shows stored labels", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "main",
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults(),
		});
		api.createPlan.mockResolvedValue({
			plans: [
				testEntry(),
				testEntry("2026-09-25.11-00-00", "scoping", "Untitled", false),
			],
			selected: { plan: "2026-09-25.11-00-00", role: "scoping" },
			config_defaults: testDefaults({ model: "stored-model", effort: null }),
		});
		const user = userEvent.setup();
		render(<App />);
		await user.click(await screen.findByRole("button", { name: "open" }));
		await screen.findByRole("button", { name: "+ NEW PLAN" });
		await user.click(screen.getByRole("button", { name: "+ NEW PLAN" }));
		await vi.waitFor(() =>
			expect(api.warmSession).toHaveBeenCalledWith({
				plan: "2026-09-25.11-00-00",
				role: "scoping",
			}),
		);
		expect(screen.getByText("MODEL")).toBeInTheDocument();
		expect(screen.getByText("EFFORT")).toBeInTheDocument();
		expect(screen.getByText("stored-model")).toBeInTheDocument();
		expect(screen.getByLabelText("Model")).toBeDisabled();
		expect(screen.getByLabelText("Effort")).toBeDisabled();
	});

	test("incoming config_options enables authoritative lists", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "main",
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults({ model: "stale-model", effort: null }),
		});
		const user = userEvent.setup();
		render(<App />);
		await user.click(await screen.findByRole("button", { name: "open" }));
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(screen.getByText("stale-model")).toBeInTheDocument();
		emit({
			type: "config_options",
			session: testKey(),
			options: authoritative(),
		});
		await screen.findByText("Real Model");
		expect(screen.queryByText("stale-model")).not.toBeInTheDocument();
		expect(screen.getByLabelText("Model")).not.toBeDisabled();
	});
});
