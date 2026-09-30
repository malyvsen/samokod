import { act, render, screen } from "@testing-library/react";
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
	finishLanding: vi.fn(),
	beginLanding: vi.fn(),
	abandonPlan: vi.fn(),
	cancelExecution: vi.fn(),
	selectPlan: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setConfigOption: vi.fn(),
	warmSession: vi.fn(),
	loadHistory: vi.fn(),
	scopingDraft: vi.fn(),
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
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "feature",
		plans: [testEntry()],
		selected: testKey(),
		config_defaults: testDefaults(),
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.scopingDraft.mockResolvedValue(null);
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

describe("history", () => {
	test("loading hint shows while history streams", async () => {
		await openChat();
		expect(api.loadHistory).toHaveBeenCalledWith(testKey());
		emit({ type: "history_begin", session: testKey() });
		expect(screen.getByText("replaying past messages")).toBeInTheDocument();
		emit({ type: "history_done", session: testKey() });
		expect(
			screen.queryByText("replaying past messages"),
		).not.toBeInTheDocument();
	});

	test("failed history shows retry and retry reloads", async () => {
		const user = await openChat();
		emit({
			type: "history_failed",
			session: testKey(),
			raw: "boom",
			hint: "history hint",
			retryable: true,
		});
		expect(screen.getByText("history hint")).toBeInTheDocument();
		expect(screen.getByText(/boom/)).toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "retry" }));
		expect(api.loadHistory).toHaveBeenCalledTimes(2);
	});

	test("non-retryable history hides retry", async () => {
		await openChat();
		emit({
			type: "history_failed",
			session: testKey(),
			raw: "no saved session found for this plan - it predates session recording or its session was pruned",
			hint: "this plan's history is unavailable - it predates session recording or was pruned",
			retryable: false,
		});
		expect(
			screen.getByText(
				"this plan's history is unavailable - it predates session recording or was pruned",
			),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});

	test("restored history renders in the transcript", async () => {
		await openChat();
		emit({ type: "history_begin", session: testKey() });
		emit({ type: "user_text", session: testKey(), chunk: "hello" });
		emit({ type: "agent_text", session: testKey(), chunk: "hi there" });
		emit({ type: "history_done", session: testKey() });
		expect(screen.getByText("hello")).toBeInTheDocument();
		expect(screen.getByText("hi there")).toBeInTheDocument();
	});
});
