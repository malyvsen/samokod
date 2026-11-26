import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry, testKey } from "./fixtures";
import {
	api,
	emitToggleSettings,
	openChat,
	stubMatchMedia,
} from "./testHarness";

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
		selected: testKey(),
		config_defaults: testDefaults(),
		scoping_preview: null,
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.planMdText.mockResolvedValue(null);
});

describe("sidebar", () => {
	test("native menu event flips between plans and settings", async () => {
		await openChat(api);
		expect(
			screen.getByRole("button", { name: "New plan" }),
		).toBeInTheDocument();
		emitToggleSettings(api);
		await screen.findByText("MODEL");
		expect(
			screen.queryByRole("button", { name: "New plan" }),
		).not.toBeInTheDocument();
		emitToggleSettings(api);
		await screen.findByRole("button", { name: "New plan" });
		expect(screen.queryByText("MODEL")).not.toBeInTheDocument();
	});

	test("repo footer shows in both modes", async () => {
		await openChat(api);
		const footer = screen.getByText("/repo · feature");
		expect(footer.className).toContain("sidebar-footer");
		expect(footer.getAttribute("title")).toBe("/repo · feature");
		emitToggleSettings(api);
		await screen.findByText("MODEL");
		const settingsFooter = screen.getByText("/repo · feature");
		expect(settingsFooter.getAttribute("title")).toBe("/repo · feature");
	});
});
