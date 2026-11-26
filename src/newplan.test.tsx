import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testDefaults, testEntry } from "./fixtures";
import { api, emitNewPlan, openChat, stubMatchMedia } from "./testHarness";

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
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "feature",
		plans: [testEntry()],
		selected: { plan: "2026-09-25.10-54-59", role: "scoping" },
		config_defaults: testDefaults(),
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.planMdText.mockResolvedValue(null);
});

describe("new-plan menu event", () => {
	test("in chat view forwards the current draft to createPlan", async () => {
		api.createPlan.mockResolvedValue({
			plans: [testEntry()],
			selected: { plan: "2026-09-25.10-54-59", role: "scoping" },
			config_defaults: testDefaults(),
		});
		const user = await openChat(api);
		await user.keyboard("hello draft");
		emitNewPlan(api);
		expect(api.createPlan).toHaveBeenCalledWith("hello draft");
	});

	test("in picker view does not call createPlan", async () => {
		render(<App />);
		await screen.findByRole("button", { name: "browse…" });
		emitNewPlan(api);
		expect(api.createPlan).not.toHaveBeenCalled();
	});
});
