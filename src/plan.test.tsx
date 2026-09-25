import { act, render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testEntry, testKey, testPlan } from "./fixtures";
import type { AppEvent } from "./types";

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
	api.getPrefs.mockResolvedValue({
		recent: [{ path: "/repo" }],
	});
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "main" });
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "main",
		plans: [testEntry()],
		selected: testKey(),
	});
});

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

async function openChat() {
	const user = userEvent.setup();
	render(<App />);
	await user.click(await screen.findByRole("button", { name: "open" }));
	await screen.findByRole("button", { name: "plan phase scoping" });
}

describe("plan", () => {
	test("session_reset keeps the plan", async () => {
		await openChat();
		emit({ type: "session_reset", session: testKey() });
		expect(
			screen.getByRole("button", { name: "plan phase scoping" }),
		).toBeInTheDocument();
	});

	test("plan_changed swaps the plan", async () => {
		await openChat();
		emit({
			type: "plan_changed",
			session: testKey(),
			plan: testPlan("executing", true),
		});
		expect(
			screen.getByRole("button", { name: "plan phase executing" }),
		).toBeInTheDocument();
	});

	test("completing clears the transcript and keeps the selection", async () => {
		api.markCompleted.mockResolvedValue({
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "completed", "Shiny feature"),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
		});
		const user = userEvent.setup();
		await openChat();
		emit({ type: "agent_text", session: testKey(), chunk: "old chat" });
		emit({ type: "turn_done", session: testKey() });
		expect(screen.getByText("old chat")).toBeInTheDocument();
		emit({
			type: "plan_changed",
			session: testKey(),
			plan: testPlan("executing", true),
		});
		await user.click(
			screen.getByRole("button", { name: "plan phase executing" }),
		);
		await user.click(screen.getByRole("button", { name: "Mark completed" }));
		expect(api.markCompleted).toHaveBeenCalledTimes(1);
		expect(screen.queryByText("old chat")).not.toBeInTheDocument();
	});

	test("abandoning clears the transcript and keeps the selection", async () => {
		api.abandonPlan.mockResolvedValue({
			plans: [testEntry("2026-09-25.10-54-59", "cancelled", "Untitled")],
			selected: testKey(),
		});
		const user = userEvent.setup();
		await openChat();
		emit({ type: "agent_text", session: testKey(), chunk: "old chat" });
		emit({ type: "turn_done", session: testKey() });
		emit({
			type: "plan_changed",
			session: testKey(),
			plan: testPlan("executing", true),
		});
		await user.click(
			screen.getByRole("button", { name: "plan phase executing" }),
		);
		await user.click(screen.getByRole("button", { name: "Abandon" }));
		expect(api.abandonPlan).toHaveBeenCalledTimes(1);
		expect(screen.queryByText("old chat")).not.toBeInTheDocument();
	});
});
