import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { OpenRepoResult, SessionKey } from "./bindings";
import {
	testDefaults,
	testEntry,
	testEntryWith,
	testKey,
	testStatus,
} from "./fixtures";
import { api, emitAppEvent, openChat, stubMatchMedia } from "./testHarness";

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
	stubMatchMedia();
	api.getPrefs.mockResolvedValue({
		recent: [{ path: "/repo" }],
	});
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	const opened: OpenRepoResult = {
		repo_root: "/repo",
		branch: "feature",
		plans: [testEntry()],
		selected: testKey(),
		config_defaults: testDefaults(),
	};
	api.openRepo.mockResolvedValue(opened);
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.sendPrompt.mockResolvedValue(undefined);
});

function userBubbles(): NodeListOf<Element> {
	return document.querySelectorAll(".msg.user:not(.draft)");
}

describe("draft bubble in chat", () => {
	test("compose box opens empty", async () => {
		await openChat(api);
		const area = await screen.findByRole("textbox", {
			name: "Ask for a change…",
		});
		expect(area.textContent).toBe("");
	});

	test("send shows one bubble", async () => {
		const user = await openChat(api);
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(screen.queryByText("TEMPLATE")).not.toBeInTheDocument();
		await user.keyboard("hello{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello");
		await screen.findByText("hello");
		expect(screen.queryByText("TEMPLATE")).not.toBeInTheDocument();
		const bubbles = userBubbles();
		expect(bubbles).toHaveLength(1);
		expect(bubbles[0]?.textContent).toContain("hello");
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
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
		});
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

	test("keeps scoping draft across click-away and forwards it to selectPlan", async () => {
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
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
		});
		await user.keyboard("hello draft");
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		expect(api.selectPlan).toHaveBeenCalledWith(
			{ plan: "bbb", role: "scoping" },
			"hello draft",
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("");
		await user.click(
			await screen.findByRole("button", {
				name: "First, scoping, opens Scoping",
			}),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("hello draft");
	});

	test("kept title renders from mocked plans", async () => {
		const kept = testEntryWith("aaa", "scoping", "hello draft title", false, [
			testStatus("scoping"),
		]);
		const second = testEntryWith("bbb", "scoping", "Second", false, [
			testStatus("scoping"),
		]);
		const before = [
			testEntryWith("aaa", "scoping", "First", false, [testStatus("scoping")]),
			second,
		];
		const after = [kept, second];
		api.selectPlan.mockImplementation(async (session: SessionKey) => ({
			plans: after,
			selected: session,
			config_defaults: testDefaults(),
		}));
		const user = await openChat(api, {
			plans: before,
			selected: { plan: "aaa", role: "scoping" },
		});
		await user.keyboard("hello draft title hello");
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		await screen.findByText("hello draft title");
	});

	test("whitespace draft discards chats and draft", async () => {
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
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
		});
		emitAppEvent(api, {
			type: "agent_text",
			session: { plan: "aaa", role: "scoping" },
			chunk: "aaa ephemeral",
		});
		emitAppEvent(api, {
			type: "turn_done",
			session: { plan: "aaa", role: "scoping" },
		});
		expect(screen.getByText("aaa ephemeral")).toBeInTheDocument();
		await user.keyboard("   ");
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		expect(api.selectPlan).toHaveBeenCalledWith(
			{ plan: "bbb", role: "scoping" },
			"",
		);
		expect(screen.queryByText("aaa ephemeral")).not.toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "First, scoping, opens Scoping",
			}),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("");
	});

	test("new plan forwards current draft and preserves it", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const kept = testEntryWith("aaa", "scoping", "hello draft title", false, [
			testStatus("scoping"),
		]);
		const fresh = testEntryWith("ccc", "scoping", "Untitled", false, [
			testStatus("scoping"),
		]);
		api.createPlan.mockImplementation(async () => ({
			plans: [kept, fresh],
			selected: { plan: "ccc", role: "scoping" },
			config_defaults: testDefaults(),
		}));
		api.selectPlan.mockImplementation(async (session: SessionKey) => ({
			plans: [kept, fresh],
			selected: session,
			config_defaults: testDefaults(),
		}));
		const user = await openChat(api, {
			plans: [first],
			selected: { plan: "aaa", role: "scoping" },
		});
		await user.keyboard("hello draft");
		await user.click(screen.getByRole("button", { name: "New plan" }));
		expect(api.createPlan).toHaveBeenCalledWith("hello draft");
		expect(await screen.findByRole("textbox")).toHaveTextContent("");
		await user.click(
			await screen.findByRole("button", {
				name: "hello draft title, scoping, opens Scoping",
			}),
		);
		expect(await screen.findByRole("textbox")).toHaveTextContent("hello draft");
	});

	test("stays visible with queue placeholder while working", async () => {
		const user = await openChat(api);
		api.sendPrompt.mockReturnValue(new Promise(() => {}));
		await user.keyboard("do it{Enter}");
		await screen.findByRole("textbox", { name: "Queue a follow-up…" });
		emitAppEvent(api, {
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "read: a", status: "in_progress", label: "read" },
		});
		expect(
			screen.getByRole("button", { name: "Stop this turn" }),
		).toBeInTheDocument();
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(
			screen.queryByRole("button", { name: "Stop this turn" }),
		).not.toBeInTheDocument();
	});

	describe("draft context", () => {
		test("live session shows context with empty draft", async () => {
			const live = testEntryWith("aaa", "scoping", "First", false, [
				testStatus("scoping", {
					live: true,
					spend: { cost: 1.24, ctx_pct: 38.4 },
				}),
			]);
			await openChat(api, {
				plans: [live],
				selected: { plan: "aaa", role: "scoping" },
			});
			const badge = screen.getByText("38% context");
			expect(badge.className).toContain("ctx-draft");
			expect(
				badge.closest(".msg.user.draft")?.querySelector(".who .right"),
			).not.toBeNull();
		});

		test("live session keeps context with draft text", async () => {
			const live = testEntryWith("aaa", "scoping", "First", false, [
				testStatus("scoping", {
					live: true,
					spend: { cost: 1.24, ctx_pct: 38.4 },
				}),
			]);
			const user = await openChat(api, {
				plans: [live],
				selected: { plan: "aaa", role: "scoping" },
			});
			await user.keyboard("hello");
			expect(await screen.findByRole("textbox")).toHaveTextContent("hello");
			expect(screen.getByText("38% context")).toBeInTheDocument();
		});

		test("non-live session shows no context", async () => {
			const idle = testEntryWith("aaa", "scoping", "First", false, [
				testStatus("scoping", {
					live: false,
					spend: { cost: 1.24, ctx_pct: 38.4 },
				}),
			]);
			await openChat(api, {
				plans: [idle],
				selected: { plan: "aaa", role: "scoping" },
			});
			expect(screen.queryByText(/context/)).toBeNull();
		});
	});
});

describe("draft helpers", () => {
	test("isBlankDraft trims", async () => {
		const { isBlankDraft } = await import("./sessions/drafts");
		expect(isBlankDraft(null)).toBe(true);
		expect(isBlankDraft(undefined)).toBe(true);
		expect(isBlankDraft("")).toBe(true);
		expect(isBlankDraft("   ")).toBe(true);
		expect(isBlankDraft("hello")).toBe(false);
	});

	test("shouldKeepOnSwitch keeps sent or drafted sessions", async () => {
		const { shouldKeepOnSwitch, hasSentUserMessage } = await import(
			"./sessions/drafts"
		);
		const { testKey } = await import("./fixtures");
		expect(hasSentUserMessage({}, testKey())).toBe(false);
		expect(shouldKeepOnSwitch({}, {}, testKey())).toBe(false);
		const drafted = {
			"2026-09-25.10-54-59::scoping": "hi",
		};
		expect(shouldKeepOnSwitch({}, drafted, testKey())).toBe(true);
	});
});
