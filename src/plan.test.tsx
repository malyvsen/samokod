import { act, render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testPlan, testSession } from "./fixtures";
import type { AppEvent } from "./types";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const api = vi.hoisted(() => ({
	getPrefs: vi.fn(),
	validateRepo: vi.fn(),
	openRepo: vi.fn(),
	executePlan: vi.fn(),
	markCompleted: vi.fn(),
	abandonPlan: vi.fn(),
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
	api.openRepo.mockResolvedValue(testSession([]));
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
		emit({ type: "session_reset" });
		expect(
			screen.getByRole("button", { name: "plan phase scoping" }),
		).toBeInTheDocument();
	});

	test("plan_changed swaps the plan", async () => {
		await openChat();
		emit({
			type: "plan_changed",
			plan: testPlan("executing", true),
		});
		expect(
			screen.getByRole("button", { name: "plan phase executing" }),
		).toBeInTheDocument();
	});
});
