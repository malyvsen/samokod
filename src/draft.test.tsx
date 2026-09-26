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
		config_defaults: testDefaults(),
	});
	api.warmSession.mockResolvedValue(undefined);
	api.scopingDraft.mockResolvedValue(null);
	api.sendPrompt.mockResolvedValue(undefined);
});

async function openChat(plans?: PlanEntry[], selected?: SessionKey) {
	if (plans !== undefined) {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "main",
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
	test("sends prefilled editable draft verbatim", async () => {
		api.scopingDraft.mockResolvedValue("Help me scope things");
		const user = await openChat();
		const area = await screen.findByRole("textbox", {
			name: "Ask for a change…",
		});
		expect(area.textContent).toBe("Help me scope things");
		expect(api.scopingDraft).toHaveBeenCalledWith(testKey());
		await user.keyboard("{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(
			testKey(),
			"Help me scope things",
		);
		await screen.findByText("Help me scope things");
	});

	test("edited template sends as edited", async () => {
		api.scopingDraft.mockResolvedValue("TEMPLATE");
		const user = await openChat();
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		await user.keyboard(" more{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "TEMPLATE more");
	});

	test("no prefill for non-fresh sessions", async () => {
		api.scopingDraft.mockResolvedValue(null);
		const user = await openChat();
		const area = await screen.findByRole("textbox", {
			name: "Ask for a change…",
		});
		expect(area.textContent).toBe("");
		expect(api.scopingDraft).toHaveBeenCalledWith(testKey());
		await user.keyboard("hello{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello");
	});

	test("preserves per-session drafts across switches", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const second = testEntryWith("bbb", "scoping", "Second", false, [
			testStatus("scoping"),
		]);
		const plans = [first, second];
		api.scopingDraft.mockImplementation(async (session: SessionKey) =>
			session.plan === "aaa" ? "DRAFT-A" : "DRAFT-B",
		);
		api.selectPlan.mockImplementation(async (session: SessionKey) => ({
			plans,
			selected: session,
			config_defaults: testDefaults(),
		}));
		const user = await openChat(plans, { plan: "aaa", role: "scoping" });
		expect(await screen.findByRole("textbox")).toHaveTextContent("DRAFT-A");
		await user.keyboard("!");
		await user.click(
			await screen.findByRole("button", { name: "Second Scoping" }),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("DRAFT-B");
		await user.keyboard("?");
		await user.click(
			await screen.findByRole("button", { name: "First Scoping" }),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("DRAFT-A!");
		expect(api.scopingDraft).toHaveBeenCalledTimes(2);
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
