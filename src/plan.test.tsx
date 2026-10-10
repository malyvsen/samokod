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
		scopingPreview: null,
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
});

function textbox(): HTMLElement {
	return screen.getByRole("textbox", { name: "Ask for a change…" });
}

describe("plan", () => {
	test("session_reset keeps the selected plan", async () => {
		await openChat(api);
		emitAppEvent(api, { type: "session_reset", session: testKey() });
		expect(screen.getByText("Parallel sessions")).toBeInTheDocument();
		expect(screen.getByText("Scoping")).toBeInTheDocument();
	});

	test("plans_changed lists both rows of an approved plan", async () => {
		await openChat(api);
		emitAppEvent(api, {
			type: "plans_changed",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
			scopingPreview: null,
		});
		expect(screen.getByText("Shiny")).toBeInTheDocument();
		expect(screen.getByText("Scoping")).toBeInTheDocument();
		expect(screen.getByText("Executing")).toBeInTheDocument();
	});

	async function openPlanMenu(user: Awaited<ReturnType<typeof openChat>>) {
		await user.click(
			await screen.findByRole("button", {
				name: "Plan options for Parallel sessions",
			}),
		);
	}

	test("menu execute shows the live first prompt without sending", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("2026-09-25.10-54-59", "scoping", "Parallel sessions", true),
			],
			selected: testKey(),
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.executePlan.mockResolvedValue({
			plans: [
				testEntry(
					"2026-09-25.10-54-59",
					"executing",
					"Parallel sessions",
					true,
				),
			],
			selected: { plan: "2026-09-25.10-54-59", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api);
		await openPlanMenu(user);
		await user.click(await screen.findByRole("menuitem", { name: "Execute" }));
		expect(api.executePlan).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59",
			role: "scoping",
		});
		expect(api.sendPrompt).not.toHaveBeenCalled();
		emitAppEvent(api, {
			type: "user_text",
			session: { plan: "2026-09-25.10-54-59", role: "executing" },
			chunk: "EXECUTING-PROMPT",
		});
		await screen.findByText("EXECUTING-PROMPT");
	});

	test("failed execute surfaces its own hint without retry", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("2026-09-25.10-54-59", "scoping", "Parallel sessions", true),
			],
			selected: testKey(),
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.executePlan.mockRejectedValue(new Error("nope"));
		const user = await openChat(api);
		await openPlanMenu(user);
		await user.click(await screen.findByRole("menuitem", { name: "Execute" }));
		expect(
			await screen.findByText("couldn't start execution - try again"),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});

	test("menu cancel cancels and keeps the transcript", async () => {
		api.cancelPlan.mockResolvedValue({
			plans: [
				testEntry("2026-09-25.10-54-59.draft-idea", "cancelled", "Draft idea"),
			],
			selected: { plan: "2026-09-25.10-54-59.draft-idea", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat(api);
		await user.keyboard("old question{Enter}");
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "old chat",
		});
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await openPlanMenu(user);
		await user.click(await screen.findByRole("menuitem", { name: "Cancel" }));
		expect(api.cancelPlan).toHaveBeenCalledTimes(1);
		await screen.findByText("Draft idea");
		expect(screen.getByText("old chat")).toBeInTheDocument();
		expect(screen.getByText("old question")).toBeInTheDocument();
	});

	test("cancelling an empty session drops its chat state", async () => {
		api.cancelPlan.mockResolvedValue({
			plans: [testEntry("bbb", "scoping", "Beta", true)],
			selected: { plan: "bbb", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api);
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "ephemeral",
		});
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		expect(screen.getByText("ephemeral")).toBeInTheDocument();
		await openPlanMenu(user);
		await user.click(await screen.findByRole("menuitem", { name: "Cancel" }));
		expect(api.cancelPlan).toHaveBeenCalledTimes(1);
		await vi.waitFor(() =>
			expect(screen.queryByText("ephemeral")).not.toBeInTheDocument(),
		);
	});

	test("first send appends the user bubble", async () => {
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat(api);
		await user.keyboard("hello plan{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "hello plan");
		expect(screen.getByText("hello plan")).toBeInTheDocument();
	});

	test("background sessions update silently", async () => {
		await openChat(api);
		emitAppEvent(api, {
			type: "agent_text",
			session: { plan: "other-plan", role: "scoping" },
			chunk: "background chat",
		});
		emitAppEvent(api, {
			type: "turn_done",
			session: { plan: "other-plan", role: "scoping" },
		});
		expect(screen.queryByText("background chat")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("background plans_changed keeps transcript and textbox", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
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
		emitAppEvent(api, {
			type: "agent_text",
			session: { plan: "bbb", role: "scoping" },
			chunk: "bbb chat",
		});
		emitAppEvent(api, {
			type: "turn_done",
			session: { plan: "bbb", role: "scoping" },
		});
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		const box = textbox();
		box.focus();
		emitAppEvent(api, {
			type: "plans_changed",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "bbb", role: "scoping" },
			scopingPreview: null,
		});
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		expect(screen.queryByText("bbb chat")).not.toBeInTheDocument();
		expect(document.activeElement).toBe(box);
		expect(textbox()).toBe(box);
	});

	test("background execute response keeps viewing", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		let resolveExecute!: (value: {
			plans: ReturnType<typeof testEntry>[];
			selected: { plan: string; role: "executing" };
			config_defaults: ReturnType<typeof testDefaults>;
			scopingPreview: string | null;
		}) => void;
		api.executePlan.mockImplementation(
			() =>
				new Promise((resolve) => {
					resolveExecute = resolve;
				}),
		);
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
		await user.click(
			await screen.findByRole("button", { name: "Plan options for Beta" }),
		);
		await user.click(await screen.findByRole("menuitem", { name: "Execute" }));
		expect(api.executePlan).toHaveBeenCalledWith({
			plan: "bbb",
			role: "scoping",
		});
		const box = textbox();
		box.focus();
		resolveExecute({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb.slug", "executing", "Beta", true),
			],
			selected: { plan: "bbb.slug", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		await screen.findByText("Beta");
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		expect(document.activeElement).toBe(box);
		expect(textbox()).toBe(box);
	});

	test("background cancel response keeps viewing", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		let resolveCancel!: (value: {
			plans: ReturnType<typeof testEntry>[];
			selected: { plan: string; role: "scoping" };
			config_defaults: ReturnType<typeof testDefaults>;
			scopingPreview: string | null;
		}) => void;
		api.cancelPlan.mockImplementation(
			() =>
				new Promise((resolve) => {
					resolveCancel = resolve;
				}),
		);
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
		await user.click(
			await screen.findByRole("button", { name: "Plan options for Beta" }),
		);
		await user.click(await screen.findByRole("menuitem", { name: "Cancel" }));
		expect(api.cancelPlan).toHaveBeenCalledWith({
			plan: "bbb",
			role: "scoping",
		});
		const box = textbox();
		box.focus();
		resolveCancel({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb.cancelled", "cancelled", "Beta"),
			],
			selected: { plan: "bbb.cancelled", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		await screen.findByText("Beta");
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		expect(document.activeElement).toBe(box);
		expect(textbox()).toBe(box);
	});

	test("viewed execute follows its own rename", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [testEntry("aaa", "scoping", "Alpha", true)],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.executePlan.mockResolvedValue({
			plans: [testEntry("aaa.slug", "executing", "Alpha", true)],
			selected: { plan: "aaa.slug", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api);
		await user.click(
			await screen.findByRole("button", { name: "Plan options for Alpha" }),
		);
		await user.click(await screen.findByRole("menuitem", { name: "Execute" }));
		await screen.findByText("Executing");
		expect(screen.getByRole("textbox")).toBeInTheDocument();
	});

	test("vanished viewed plan falls back to most-recent", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
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
		emitAppEvent(api, {
			type: "agent_text",
			session: { plan: "bbb", role: "scoping" },
			chunk: "bbb chat",
		});
		emitAppEvent(api, {
			type: "turn_done",
			session: { plan: "bbb", role: "scoping" },
		});
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		emitAppEvent(api, {
			type: "plans_changed",
			plans: [testEntry("bbb", "scoping", "Beta", true)],
			selected: { plan: "bbb", role: "scoping" },
			scopingPreview: null,
		});
		await screen.findByText("bbb chat");
		expect(screen.queryByText("aaa chat")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("background turn keeps focus and transcript", async () => {
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
		expect(textbox()).toBe(box);
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
		expect(textbox()).toBeInTheDocument();
	});

	test("background spend_tick refresh keeps focus", async () => {
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
			scopingPreview: null,
		});
		expect(document.activeElement).toBe(box);
		expect(textbox()).toBeInTheDocument();
	});

	test("selecting a row swaps transcript while scoping shows the plan", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.selectPlan.mockImplementation(async (session: unknown) => ({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: session,
			config_defaults: testDefaults(),
			scopingPreview: null,
		}));
		api.sendPrompt.mockResolvedValue(undefined);
		const user = await openChat(api);
		const aaa = { plan: "aaa", role: "scoping" } as const;
		const bbb = { plan: "bbb", role: "scoping" } as const;
		await user.keyboard("aaa question{Enter}");
		emitAppEvent(api, { type: "agent_text", session: aaa, chunk: "aaa chat" });
		emitAppEvent(api, { type: "turn_done", session: aaa });
		emitAppEvent(api, { type: "agent_text", session: bbb, chunk: "bbb chat" });
		emitAppEvent(api, { type: "turn_done", session: bbb });
		expect(screen.getByText("aaa chat")).toBeInTheDocument();
		await user.click(
			screen.getByRole("button", { name: "Beta, scoping, opens Scoping" }),
		);
		expect(api.selectPlan).toHaveBeenCalledWith(bbb, null);
		await screen.findByText("bbb chat");
		expect(screen.queryByText("aaa chat")).not.toBeInTheDocument();
		expect(screen.getByText("No plan yet.")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).not.toBeInTheDocument();
		await user.click(
			screen.getByRole("button", { name: "Alpha, scoping, opens Scoping" }),
		);
		await screen.findByText("aaa chat");
	});

	test("selecting away drops only the empty previous session", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.selectPlan.mockResolvedValue({
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "bbb", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api);
		const aaa = { plan: "aaa", role: "scoping" } as const;
		emitAppEvent(api, {
			type: "agent_text",
			session: aaa,
			chunk: "aaa ephemeral",
		});
		emitAppEvent(api, { type: "turn_done", session: aaa });
		expect(screen.getByText("aaa ephemeral")).toBeInTheDocument();
		await user.click(
			screen.getByRole("button", { name: "Beta, scoping, opens Scoping" }),
		);
		expect(api.selectPlan).toHaveBeenCalledWith(
			{
				plan: "bbb",
				role: "scoping",
			},
			null,
		);
		expect(screen.queryByText("aaa ephemeral")).not.toBeInTheDocument();
		expect(
			await screen.findByRole("textbox", { name: "Ask for a change…" }),
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
			scopingPreview: null,
		});
		const user = await openChat(api);
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "old chat",
		});
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await user.click(screen.getByRole("button", { name: "New plan" }));
		expect(api.createPlan).toHaveBeenCalledTimes(1);
		expect(screen.queryByText("old chat")).not.toBeInTheDocument();
		expect(
			await screen.findByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
	});

	test("failed turns show a red pill and retry resends", async () => {
		api.retryLast.mockResolvedValue(true);
		const user = await openChat(api);
		emitAppEvent(api, {
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

	test("failed select surfaces an error and keeps selection", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("aaa", "scoping", "Alpha", true),
				testEntry("bbb", "scoping", "Beta", true),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.selectPlan.mockRejectedValue(new Error("gone"));
		const user = await openChat(api);
		await user.click(
			screen.getByRole("button", { name: "Beta, scoping, opens Scoping" }),
		);
		expect(
			await screen.findByText("couldn't switch plan - try again"),
		).toBeInTheDocument();
		expect(screen.getByText(/gone/)).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
		expect(screen.getByText("Alpha")).toBeInTheDocument();
	});

	test("failed create surfaces its own hint without retry", async () => {
		api.createPlan.mockRejectedValue(new Error("denied"));
		const user = await openChat(api);
		await user.click(screen.getByRole("button", { name: "New plan" }));
		expect(
			await screen.findByText("couldn't create plan - try again"),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});

	test("failed cancel surfaces its own hint without retry", async () => {
		api.cancelPlan.mockRejectedValue(new Error("denied"));
		const user = await openChat(api);
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "old chat",
		});
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await openPlanMenu(user);
		await user.click(await screen.findByRole("menuitem", { name: "Cancel" }));
		expect(
			await screen.findByText("couldn't cancel plan - try again"),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});

	async function openShinyMenu(user: Awaited<ReturnType<typeof openChat>>) {
		await user.click(
			await screen.findByRole("button", {
				name: "Plan options for Shiny",
			}),
		);
	}

	test("mode switch flips the plan to manual", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.setPlanMode.mockResolvedValue({
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api);
		await openShinyMenu(user);
		await user.click(
			await screen.findByRole("menuitemcheckbox", { name: "Auto-advance" }),
		);
		expect(api.setPlanMode).toHaveBeenCalledWith(
			"2026-09-25.10-54-59.slug",
			true,
		);
	});

	test("failed mode switch surfaces its own hint without retry", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.setPlanMode.mockRejectedValue(new Error("denied"));
		const user = await openChat(api);
		await openShinyMenu(user);
		await user.click(
			await screen.findByRole("menuitemcheckbox", { name: "Auto-advance" }),
		);
		expect(
			await screen.findByText("couldn't switch plan mode - try again"),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});

	test("evergreen toggle skips cleanup for the plan", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.setPlanEvergreen.mockResolvedValue({
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api);
		await openShinyMenu(user);
		await user.click(
			await screen.findByRole("menuitemcheckbox", { name: "Evergreen" }),
		);
		expect(api.setPlanEvergreen).toHaveBeenCalledWith(
			"2026-09-25.10-54-59.slug",
			false,
		);
	});

	test("failed evergreen switch surfaces its own hint without retry", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		api.setPlanEvergreen.mockRejectedValue(new Error("denied"));
		const user = await openChat(api);
		await openShinyMenu(user);
		await user.click(
			await screen.findByRole("menuitemcheckbox", { name: "Evergreen" }),
		);
		expect(
			await screen.findByText("couldn't switch evergreen intent - try again"),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});
});
