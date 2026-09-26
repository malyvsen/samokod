import { act, render, screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testDefaults, testEntry, testKey } from "./fixtures";
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
	selectPlan: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setConfigOption: vi.fn(),
	warmSession: vi.fn(),
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
		config_defaults: testDefaults(),
	});
	api.warmSession.mockResolvedValue(undefined);
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
			config_defaults: testDefaults(),
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
			config_defaults: testDefaults(),
		});
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat();
		await user.keyboard("old question{Enter}");
		emit({ type: "agent_text", session: testKey(), chunk: "old chat" });
		emit({ type: "turn_done", session: testKey() });
		const scoping = screen.getByRole("button", {
			name: "Parallel sessions Scoping",
		});
		const row = scoping.closest(".session");
		if (row === null) throw new Error("scoping row missing");
		const button = await within(row as HTMLElement).findByRole("button", {
			name: "Abandon 2026-09-25.10-54-59",
		});
		await user.click(button);
		expect(api.abandonPlan).toHaveBeenCalledTimes(1);
		await screen.findByText("Draft idea");
		expect(screen.getByText("old chat")).toBeInTheDocument();
		expect(screen.getByText("old question")).toBeInTheDocument();
	});

	test("abandoning an empty session drops its chat state", async () => {
		api.abandonPlan.mockResolvedValue({
			plans: [testEntry("bbb", "scoping", "Beta", true)],
			selected: { plan: "bbb", role: "scoping" },
			config_defaults: testDefaults(),
		});
		const user = await openChat();
		emit({ type: "agent_text", session: testKey(), chunk: "ephemeral" });
		emit({ type: "turn_done", session: testKey() });
		expect(screen.getByText("ephemeral")).toBeInTheDocument();
		const scoping = screen.getByRole("button", {
			name: "Parallel sessions Scoping",
		});
		const row = scoping.closest(".session");
		if (row === null) throw new Error("scoping row missing");
		const button = await within(row as HTMLElement).findByRole("button", {
			name: "Abandon 2026-09-25.10-54-59",
		});
		await user.click(button);
		expect(api.abandonPlan).toHaveBeenCalledTimes(1);
		await vi.waitFor(() =>
			expect(screen.queryByText("ephemeral")).not.toBeInTheDocument(),
		);
	});

	test("first send appends the user bubble", async () => {
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat();
		await user.keyboard("hello plan{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello plan");
		expect(screen.getByText("hello plan")).toBeInTheDocument();
	});

	test("background sessions update silently", async () => {
		await openChat();
		emit({
			type: "agent_text",
			session: { plan: "other-plan", role: "scoping" },
			chunk: "background chat",
		});
		emit({
			type: "turn_done",
			session: { plan: "other-plan", role: "scoping" },
		});
		expect(screen.queryByText("background chat")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("selecting a row swaps transcript, todos, and cost", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "main",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
		});
		api.selectPlan.mockImplementation(async (session: unknown) => ({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: session,
			config_defaults: testDefaults(),
		}));
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat();
		const aaa = { plan: "aaa", role: "scoping" } as const;
		const bbb = { plan: "bbb", role: "scoping" } as const;
		await user.keyboard("aaa question{Enter}");
		emit({ type: "agent_text", session: aaa, chunk: "aaa chat" });
		emit({ type: "turn_done", session: aaa });
		emit({ type: "agent_text", session: bbb, chunk: "bbb chat" });
		emit({ type: "turn_done", session: bbb });
		emit({
			type: "todos_changed",
			session: bbb,
			todos: [{ content: "Beta todo", status: "pending", priority: "high" }],
			changes: [],
		});
		emit({ type: "spend_tick", session: bbb, cost: 1.5, ctx_pct: 10 });
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "Beta Scoping" }));
		expect(api.selectPlan).toHaveBeenCalledWith(bbb);
		await screen.findByText("bbb chat");
		expect(screen.queryByText("aaa chat")).not.toBeInTheDocument();
		expect(screen.getByText("Beta todo")).toBeInTheDocument();
		expect(screen.getByText("$1.50")).toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "Alpha Scoping" }));
		await screen.findByText("aaa chat");
		expect(screen.queryByText("Beta todo")).not.toBeInTheDocument();
	});

	test("selecting away drops only the empty previous session", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "main",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
		});
		api.selectPlan.mockResolvedValue({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "bbb", role: "scoping" },
			config_defaults: testDefaults(),
		});
		const user = await openChat();
		const aaa = { plan: "aaa", role: "scoping" } as const;
		emit({ type: "agent_text", session: aaa, chunk: "aaa ephemeral" });
		emit({ type: "turn_done", session: aaa });
		expect(screen.getByText("aaa ephemeral")).toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "Beta Scoping" }));
		expect(api.selectPlan).toHaveBeenCalledWith({
			plan: "bbb",
			role: "scoping",
		});
		expect(screen.queryByText("aaa ephemeral")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("new plan selects a fresh empty session", async () => {
		api.createPlan.mockResolvedValue({
			plans: [
				testEntry(),
				testEntry("2026-09-25.11-00-00", "scoping", "Untitled", false),
			],
			selected: { plan: "2026-09-25.11-00-00", role: "scoping" },
			config_defaults: testDefaults(),
		});
		const user = await openChat();
		emit({ type: "agent_text", session: testKey(), chunk: "old chat" });
		emit({ type: "turn_done", session: testKey() });
		await user.click(screen.getByRole("button", { name: "+ NEW PLAN" }));
		expect(api.createPlan).toHaveBeenCalledTimes(1);
		expect(screen.queryByText("old chat")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("failed turns show a red pill and retry resends", async () => {
		api.retryLast.mockResolvedValue(true);
		const user = await openChat();
		emit({
			type: "turn_failed",
			session: testKey(),
			raw: "boom",
			hint: "retry the turn",
			retryable: true,
		});
		expect(screen.getByText("● FAILED")).toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "retry" }));
		expect(api.retryLast).toHaveBeenCalledWith(testKey());
	});
});
