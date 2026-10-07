import { renderHook, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
	testDefaults,
	testEntry,
	testEntryWith,
	testKey,
	testStatus,
} from "./fixtures";
import { clearPreviewCache } from "./sessions/preview";
import { queueReducer, useSessionQueues } from "./sessions/queue";
import { api, emitAppEvent, openChat, stubMatchMedia } from "./testHarness";
import type { SessionKey } from "./types";

vi.mock("./api", async () => {
	const { api } = await import("./testHarness");
	return api;
});

vi.mock("@tauri-apps/plugin-dialog", async () => {
	const { dialog } = await import("./testHarness");
	return dialog;
});

beforeEach(() => {
	vi.clearAllMocks();
	clearPreviewCache();
	stubMatchMedia();
	api.getPrefs.mockResolvedValue({ recent: [{ path: "/repo" }] });
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
	api.scopingTemplate.mockResolvedValue(null);
	api.sendPrompt.mockResolvedValue(undefined);
	api.cancelTurn.mockResolvedValue(undefined);
});

async function startWorking() {
	api.sendPrompt.mockReturnValue(new Promise(() => {}));
	const user = await openChat(api);
	await user.keyboard("do it{Enter}");
	await screen.findByRole("textbox", { name: "Queue a follow-up…" });
	expect(api.sendPrompt).toHaveBeenCalledTimes(1);
	return user;
}

describe("message queue", () => {
	test("composer stays live with queue placeholder while working", async () => {
		await startWorking();
		expect(screen.getByRole("button", { name: "STOP" })).toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Queue a follow-up…" }),
		).toBeInTheDocument();
	});

	test("enter enqueues without calling sendPrompt", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
	});

	test("turn_done sends the head as a YOU bubble", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "followup"),
		);
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
	});

	test("drains FIFO across two turns", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		const queued = screen.getAllByText("QUEUED");
		expect(queued).toHaveLength(2);
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "first"),
		);
		expect(screen.getAllByText("QUEUED")).toHaveLength(1);
		expect(screen.getByText("second")).toBeInTheDocument();
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "second"),
		);
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
	});

	test("turn_failed holds the queue", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		emitAppEvent(api, {
			type: "turn_failed",
			session: testKey(),
			raw: "boom",
			hint: "retry the turn",
			retryable: true,
		});
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
	});

	test("STOP keeps the queue and sends the head next", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByRole("button", { name: "STOP" }));
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "followup"),
		);
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
	});

	test("approval never auto-drains", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		emitAppEvent(api, {
			type: "permission_asked",
			session: testKey(),
			permission: {
				tool_call_id: "t1",
				title: "Run command",
				kind: "bash",
				options: [
					{ id: "allow", kind: "allow" },
					{ id: "reject", kind: "reject" },
				],
				rule_hint: "hint",
			},
		});
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
	});

	test("history replay never auto-drains", async () => {
		const user = await openChat(api);
		emitAppEvent(api, { type: "history_begin", session: testKey() });
		await screen.findByRole("textbox", { name: "Queue a follow-up…" });
		await user.keyboard("followup{Enter}");
		expect(api.sendPrompt).not.toHaveBeenCalled();
		emitAppEvent(api, { type: "history_done", session: testKey() });
		expect(api.sendPrompt).not.toHaveBeenCalled();
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
	});

	test("queue is per-session across plan switches", async () => {
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
		api.sendPrompt.mockReturnValue(new Promise(() => {}));
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
		});
		await user.keyboard("do it{Enter}");
		await screen.findByRole("textbox", { name: "Queue a follow-up…" });
		await user.keyboard("aaa followup{Enter}");
		expect(screen.getByText("aaa followup")).toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		expect(screen.queryByText("aaa followup")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "First, scoping, opens Scoping",
			}),
		);
		expect(screen.getByText("aaa followup")).toBeInTheDocument();
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
	});
});

describe("queueReducer", () => {
	test("enqueue appends per session", () => {
		const next = queueReducer(
			{},
			{
				type: "enqueue",
				sessionId: "a",
				message: { id: "1", text: "hello" },
			},
		);
		expect(next.a).toEqual([{ id: "1", text: "hello" }]);
	});

	test("remove drops the item", () => {
		const start = { a: [{ id: "1", text: "hello" }] };
		const next = queueReducer(start, {
			type: "remove",
			sessionId: "a",
			id: "1",
		});
		expect(next.a).toBeUndefined();
	});

	test("setText rewrites the item", () => {
		const start = { a: [{ id: "1", text: "hello" }] };
		const next = queueReducer(start, {
			type: "setText",
			sessionId: "a",
			id: "1",
			text: "world",
		});
		expect(next.a).toEqual([{ id: "1", text: "world" }]);
	});

	test("unknown session yields empty items", () => {
		const { result } = renderHook(() => useSessionQueues(testKey()));
		expect(result.current.items).toEqual([]);
	});
});
