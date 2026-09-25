import { render, screen } from "@testing-library/react";
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

function emit(event: AppEvent) {
	type Handler = (event: AppEvent) => void;
	const registrations = api.onAppEvent.mock.calls as unknown as Handler[][];
	const handler = registrations[0]?.[0];
	if (handler === undefined) throw new Error("no app event handler");
	handler(event);
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

async function openChat() {
	const user = userEvent.setup();
	render(<App />);
	await user.click(await screen.findByRole("button", { name: "open" }));
	await screen.findByRole("textbox", { name: "Ask for a change…" });
	return user;
}

describe("draft bubble in chat", () => {
	test("sends typed text as a user message", async () => {
		const user = await openChat();
		api.sendPrompt.mockResolvedValue(undefined);
		await user.keyboard("move the picker{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "move the picker");
		await screen.findByText("move the picker");
	});

	test("hides while working and returns on turn done", async () => {
		const user = await openChat();
		api.sendPrompt.mockReturnValue(new Promise(() => {}));
		await user.keyboard("do it{Enter}");
		expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
		expect(screen.getByRole("button", { name: "STOP" })).toBeInTheDocument();
		emit({ type: "turn_done", session: testKey() });
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});
});
