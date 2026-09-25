import { act, render, screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testEntry, testKey } from "./fixtures";
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
	await screen.findByRole("button", { name: "+ NEW PLAN" });
	return user;
}

describe("plan", () => {
	test("session_reset keeps the selected plan", async () => {
		await openChat();
		emit({ type: "session_reset", session: testKey() });
		expect(screen.getByText("Parallel sessions")).toBeInTheDocument();
		expect(screen.getByText("Scoping")).toBeInTheDocument();
	});

	test("plans_changed lists both rows of an approved plan", async () => {
		await openChat();
		emit({
			type: "plans_changed",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
		});
		expect(screen.getByText("Shiny")).toBeInTheDocument();
		expect(screen.getByText("Scoping")).toBeInTheDocument();
		expect(screen.getByText("Execution")).toBeInTheDocument();
	});

	test("row done button completes and keeps the transcript", async () => {
		api.markCompleted.mockResolvedValue({
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "completed", "Shiny feature"),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
		});
		await openChat();
		emit({
			type: "plans_changed",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
		});
		emit({
			type: "agent_text",
			session: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
			chunk: "executor chat",
		});
		emit({
			type: "turn_done",
			session: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
		});
		const execution = screen.getByRole("button", {
			name: "Shiny Execution",
		});
		const row = execution.closest(".session");
		if (row === null) throw new Error("execution row missing");
		await within(row as HTMLElement)
			.findByRole("button", { name: "Mark 2026-09-25.10-54-59.slug done" })
			.then((button) => button.click());
		expect(api.markCompleted).toHaveBeenCalledTimes(1);
		expect(screen.getByText("executor chat")).toBeInTheDocument();
	});

	test("row abandon button abandons and keeps the transcript", async () => {
		api.abandonPlan.mockResolvedValue({
			plans: [
				testEntry("2026-09-25.10-54-59.draft-idea", "cancelled", "Draft idea"),
			],
			selected: { plan: "2026-09-25.10-54-59.draft-idea", role: "scoping" },
		});
		await openChat();
		emit({ type: "agent_text", session: testKey(), chunk: "old chat" });
		emit({ type: "turn_done", session: testKey() });
		const scoping = screen.getByRole("button", {
			name: "Parallel sessions Scoping",
		});
		const row = scoping.closest(".session");
		if (row === null) throw new Error("scoping row missing");
		await within(row as HTMLElement)
			.findByRole("button", { name: "Abandon 2026-09-25.10-54-59" })
			.then((button) => button.click());
		expect(api.abandonPlan).toHaveBeenCalledTimes(1);
		expect(screen.getByText("old chat")).toBeInTheDocument();
	});
});
