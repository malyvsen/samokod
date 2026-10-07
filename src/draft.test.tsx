import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
	testDefaults,
	testEntry,
	testEntryWith,
	testKey,
	testStatus,
} from "./fixtures";
import { clearPreviewCache } from "./sessions/preview";
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

describe("draft bubble in chat", () => {
	test("compose box opens empty", async () => {
		await openChat(api);
		const area = await screen.findByRole("textbox", {
			name: "Ask for a change…",
		});
		expect(area.textContent).toBe("");
	});

	test("fresh scoping session previews its template above the draft", async () => {
		api.scopingTemplate.mockResolvedValue("TEMPLATE");
		await openChat(api);
		await screen.findByText("TEMPLATE");
		const box = screen.getByRole("textbox", { name: "Ask for a change…" });
		expect(box).toBeInTheDocument();
		const leads = document.querySelectorAll(".msg.user:not(.draft)");
		expect(leads).toHaveLength(1);
		expect(leads[0]?.textContent).toContain("TEMPLATE");
		const lead = screen.getByText("TEMPLATE").closest(".msg.user");
		const draft = box.closest(".msg.user");
		if (lead === null || draft === null) throw new Error("bubbles missing");
		expect(
			lead.compareDocumentPosition(draft) & Node.DOCUMENT_POSITION_FOLLOWING,
		).toBeTruthy();
	});

	test("send with preview appends only the user bubble and sends pure text", async () => {
		api.scopingTemplate.mockResolvedValue("TEMPLATE");
		const user = await openChat(api);
		await screen.findByText("TEMPLATE");
		await user.keyboard("hello{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello");
		await screen.findByText("hello");
		const bubbles = document.querySelectorAll(".msg.user:not(.draft)");
		expect(bubbles).toHaveLength(2);
		expect(bubbles[0]?.textContent).toContain("TEMPLATE");
		expect(bubbles[1]?.textContent).toContain("hello");
		expect(api.scopingTemplate).toHaveBeenCalledTimes(1);
	});

	test("live send falls back to one bubble when template fetch fails", async () => {
		api.scopingTemplate.mockRejectedValue(new Error("boom"));
		const user = await openChat(api);
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(screen.queryByText("TEMPLATE")).not.toBeInTheDocument();
		await user.keyboard("hello{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello");
		await screen.findByText("hello");
		expect(screen.queryByText("TEMPLATE")).not.toBeInTheDocument();
		const bubbles = document.querySelectorAll(".msg.user:not(.draft)");
		expect(bubbles).toHaveLength(1);
		expect(bubbles[0]?.textContent).toContain("hello");
	});

	test("previews stay per-session across switches", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const second = testEntryWith("bbb", "scoping", "Second", false, [
			testStatus("scoping"),
		]);
		const plans = [first, second];
		api.scopingTemplate.mockImplementation(async (session: SessionKey) =>
			session.plan === "aaa" ? "AAA-TEMPLATE" : "BBB-TEMPLATE",
		);
		api.selectPlan.mockImplementation(async (session: SessionKey) => ({
			plans,
			selected: session,
			config_defaults: testDefaults(),
		}));
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
		});
		await screen.findByText("AAA-TEMPLATE");
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		await screen.findByText("BBB-TEMPLATE");
		expect(screen.queryByText("AAA-TEMPLATE")).not.toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "First, scoping, opens Scoping",
			}),
		);
		await screen.findByText("AAA-TEMPLATE");
		expect(screen.queryByText("BBB-TEMPLATE")).not.toBeInTheDocument();
	});

	test("replayed template never duplicates the preview", async () => {
		api.scopingTemplate.mockResolvedValue(null);
		await openChat(api);
		emitAppEvent(api, { type: "history_preparing", session: testKey() });
		emitAppEvent(api, { type: "history_begin", session: testKey() });
		emitAppEvent(api, {
			type: "user_text",
			session: testKey(),
			chunk: "TEMPLATE",
		});
		emitAppEvent(api, {
			type: "user_text",
			session: testKey(),
			chunk: "hello",
		});
		emitAppEvent(api, { type: "history_done", session: testKey() });
		await screen.findByText("hello");
		expect(screen.getAllByText("TEMPLATE")).toHaveLength(1);
		expect(screen.getAllByText("hello")).toHaveLength(1);
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

	test("hides while working and returns on turn done", async () => {
		const user = await openChat(api);
		api.sendPrompt.mockReturnValue(new Promise(() => {}));
		await user.keyboard("do it{Enter}");
		expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
		expect(screen.getByRole("button", { name: "STOP" })).toBeInTheDocument();
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});
});
