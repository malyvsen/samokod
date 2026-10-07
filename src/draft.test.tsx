import { render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import {
	testDefaults,
	testEntry,
	testEntryWith,
	testKey,
	testStatus,
} from "./fixtures";
import type { AppEvent, PlanEntry, SessionKey } from "./types";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const api = vi.hoisted(() => ({
	getPrefs: vi.fn(),
	validateRepo: vi.fn(),
	openRepo: vi.fn(),
	createPlan: vi.fn(),
	executePlan: vi.fn(),
	finishLanding: vi.fn(),
	cancelPlan: vi.fn(),
	selectPlan: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setConfigOption: vi.fn(),
	warmSession: vi.fn(),
	loadHistory: vi.fn(),
	scopingTemplate: vi.fn(),
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
	api.scopingTemplate.mockResolvedValue("TEMPLATE");
	api.sendPrompt.mockResolvedValue(undefined);
});

async function openChat(plans?: PlanEntry[], selected?: SessionKey) {
	if (plans !== undefined) {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans,
			selected: selected ?? testKey(plans[0]?.name ?? "a"),
			config_defaults: testDefaults(),
		});
	}
	const user = userEvent.setup();
	render(<App />);
	await user.click(await screen.findByRole("button", { name: "open" }));
	await screen.findByRole("textbox", { name: "Ask for a change…" });
	return user;
}

describe("draft bubble in chat", () => {
	test("compose box opens empty", async () => {
		await openChat();
		const area = await screen.findByRole("textbox", {
			name: "Ask for a change…",
		});
		expect(area.textContent).toBe("");
	});

	test("live send shows template and user bubbles and sends pure text", async () => {
		api.scopingTemplate.mockResolvedValue("TEMPLATE");
		const user = await openChat();
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		await user.keyboard("hello{Enter}");
		expect(api.scopingTemplate).toHaveBeenCalledWith(testKey());
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello");
		await screen.findByText("TEMPLATE");
		await screen.findByText("hello");
	});

	test("live send falls back to one bubble when template fetch fails", async () => {
		api.scopingTemplate.mockRejectedValue(new Error("boom"));
		const user = await openChat();
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		await user.keyboard("hello{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello");
		await screen.findByText("hello");
		expect(screen.queryByText("TEMPLATE")).not.toBeInTheDocument();
	});

	test("preserves per-session user drafts across switches", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const second = testEntryWith("bbb", "scoping", "Second", false, [
			testStatus("scoping"),
		]);
		const plans = [first, second];
		api.selectPlan.mockImplementation(async (session: SessionKey) => ({
			plans,
			selected: session,
			config_defaults: testDefaults(),
		}));
		const user = await openChat(plans, { plan: "aaa", role: "scoping" });
		expect(await screen.findByRole("textbox")).toHaveTextContent("");
		await user.keyboard("hello");
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("");
		await user.keyboard("world");
		await user.click(
			await screen.findByRole("button", {
				name: "First, scoping, opens Scoping",
			}),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("hello");
	});

	test("execute sends hidden role without a prompt", async () => {
		const entry = testEntryWith("aaa", "scoping", "First", true, [
			testStatus("scoping"),
		]);
		api.executePlan.mockResolvedValue({
			plans: [entry],
			selected: { plan: "aaa", role: "executing" },
			config_defaults: testDefaults(),
		});
		const user = await openChat([entry], { plan: "aaa", role: "scoping" });
		await user.click(
			await screen.findByRole("button", {
				name: "Send aaa to execution",
			}),
		);
		expect(api.executePlan).toHaveBeenCalledWith({
			plan: "aaa",
			role: "scoping",
		});
		expect(api.sendPrompt).not.toHaveBeenCalled();
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
