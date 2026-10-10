import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry, testKey } from "./fixtures";
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
});

function textbox(): HTMLElement {
	return screen.getByRole("textbox", { name: "Ask for a change…" });
}

describe("plan focus", () => {
	test("background turn_done keeps focus and transcript", async () => {
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat(api);
		await user.keyboard("viewed question{Enter}");
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "viewed chat",
		});
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		const box = textbox();
		box.focus();
		expect(document.activeElement).toBe(box);
		emitAppEvent(api, {
			type: "agent_text",
			session: { plan: "other-plan", role: "scoping" },
			chunk: "background chat",
		});
		emitAppEvent(api, {
			type: "turn_done",
			session: { plan: "other-plan", role: "scoping" },
		});
		expect(screen.getByText("viewed chat")).toBeInTheDocument();
		expect(screen.queryByText("background chat")).not.toBeInTheDocument();
		expect(document.activeElement).toBe(box);
		expect(screen.getByRole("textbox", { name: "Ask for a change…" })).toBe(
			box,
		);
	});

	test("background permission keeps focus", async () => {
		await openChat(api);
		const box = textbox();
		box.focus();
		emitAppEvent(api, {
			type: "permission_asked",
			session: { plan: "other-plan", role: "scoping" },
			permission: {
				tool_call_id: "t1",
				title: "Run",
				tool: "bash",
				options: [{ id: "allow", kind: "allow" }],
				rule_hint: "",
			},
		});
		expect(document.activeElement).toBe(box);
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("background plans_changed with stolen selected does not remount", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
		});
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat(api);
		await user.keyboard("aaa question{Enter}");
		emitAppEvent(api, {
			type: "agent_text",
			session: { plan: "aaa", role: "scoping" },
			chunk: "aaa chat",
		});
		emitAppEvent(api, {
			type: "turn_done",
			session: { plan: "aaa", role: "scoping" },
		});
		const box = textbox();
		box.focus();
		emitAppEvent(api, {
			type: "plans_changed",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "bbb", role: "scoping" },
		});
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		expect(document.activeElement).toBe(box);
		expect(screen.getByRole("textbox", { name: "Ask for a change…" })).toBe(
			box,
		);
	});

	test("background spend_tick plans refresh keeps focus", async () => {
		await openChat(api);
		const box = textbox();
		box.focus();
		emitAppEvent(api, {
			type: "spend_tick",
			session: { plan: "other-plan", role: "scoping" },
			cost: 1.24,
			ctx_pct: 38.4,
		});
		emitAppEvent(api, {
			type: "plans_changed",
			plans: [testEntry()],
			selected: { plan: "other-plan", role: "scoping" },
		});
		expect(document.activeElement).toBe(box);
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("background execute response keeps focus", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
		});
		let resolveExecute!: (value: {
			plans: ReturnType<typeof testEntry>[];
			selected: { plan: string; role: "executing" };
			config_defaults: ReturnType<typeof testDefaults>;
		}) => void;
		api.executePlan.mockImplementation(
			() =>
				new Promise((resolve) => {
					resolveExecute = resolve;
				}),
		);
		const user = await openChat(api);
		await user.click(
			await screen.findByRole("button", { name: "Plan options for Beta" }),
		);
		await user.click(await screen.findByRole("menuitem", { name: "Execute" }));
		expect(api.executePlan).toHaveBeenCalled();
		const box = textbox();
		box.focus();
		resolveExecute({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb.slug", "executing", "Beta", true),
			],
			selected: { plan: "bbb.slug", role: "executing" },
			config_defaults: testDefaults(),
		});
		await screen.findByText("Beta");
		expect(document.activeElement).toBe(box);
		expect(screen.getByRole("textbox", { name: "Ask for a change…" })).toBe(
			box,
		);
	});

	test("background cancel response keeps focus", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
		});
		let resolveCancel!: (value: {
			plans: ReturnType<typeof testEntry>[];
			selected: { plan: string; role: "scoping" };
			config_defaults: ReturnType<typeof testDefaults>;
		}) => void;
		api.cancelPlan.mockImplementation(
			() =>
				new Promise((resolve) => {
					resolveCancel = resolve;
				}),
		);
		const user = await openChat(api);
		await user.click(
			await screen.findByRole("button", { name: "Plan options for Beta" }),
		);
		await user.click(await screen.findByRole("menuitem", { name: "Cancel" }));
		expect(api.cancelPlan).toHaveBeenCalled();
		const box = textbox();
		box.focus();
		resolveCancel({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb.cancelled", "cancelled", "Beta"),
			],
			selected: { plan: "bbb.cancelled", role: "scoping" },
			config_defaults: testDefaults(),
		});
		await screen.findByText("Beta");
		expect(document.activeElement).toBe(box);
		expect(screen.getByRole("textbox", { name: "Ask for a change…" })).toBe(
			box,
		);
	});
});
