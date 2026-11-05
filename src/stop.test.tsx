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
	api.getPrefs.mockResolvedValue({ recent: [{ path: "/repo" }] });
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.sendPrompt.mockResolvedValue(undefined);
	api.cancelTurn.mockResolvedValue(undefined);
	api.setPlanMode.mockResolvedValue({
		plans: [testEntry()],
		selected: testKey(),
		config_defaults: testDefaults(),
		scoping_preview: null,
	});
});

async function startWorkingExecuting() {
	const name = "2026-09-25.10-54-59.slug";
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "feature",
		plans: [testEntry(name, "executing", "Shiny", true)],
		selected: { plan: name, role: "executing" },
		config_defaults: testDefaults(),
		scoping_preview: null,
	});
	api.sendPrompt.mockReturnValue(new Promise(() => {}));
	const user = await openChat(api);
	await user.keyboard("do it{Enter}");
	emitAppEvent(api, {
		type: "tool_line",
		session: { plan: name, role: "executing" },
		line: { id: "t1", text: "read: a", status: "in_progress", label: "read" },
	});
	await screen.findByRole("button", { name: "Stop this turn" });
	return user;
}

describe("stop", () => {
	test("STOP on work plan shows manual note without separate mode call", async () => {
		const user = await startWorkingExecuting();
		await user.click(screen.getByRole("button", { name: "Stop this turn" }));
		expect(api.cancelTurn).toHaveBeenCalledTimes(1);
		expect(api.setPlanMode).not.toHaveBeenCalled();
		expect(
			await screen.findByText(
				"stopped - switched to manual, flip back to auto to resume",
			),
		).toBeInTheDocument();
	});

	test("STOP on scoping shows no manual note", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults(),
			scoping_preview: null,
		});
		api.sendPrompt.mockReturnValue(new Promise(() => {}));
		const user = await openChat(api);
		await user.keyboard("do it{Enter}");
		emitAppEvent(api, {
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "read: a", status: "in_progress", label: "read" },
		});
		await screen.findByRole("button", { name: "Stop this turn" });
		await user.click(screen.getByRole("button", { name: "Stop this turn" }));
		expect(api.cancelTurn).toHaveBeenCalledTimes(1);
		expect(api.setPlanMode).not.toHaveBeenCalled();
		expect(
			screen.queryByText(
				"stopped - switched to manual, flip back to auto to resume",
			),
		).not.toBeInTheDocument();
	});
});
