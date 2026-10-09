import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry, testKey } from "./fixtures";
import { api, emitAppEvent, openChat, stubMatchMedia } from "./testHarness";
import type { ConfigOptionView } from "./types";

vi.mock("./api", async () => {
	const { api } = await import("./testHarness");
	return api;
});

vi.mock("@tauri-apps/plugin-dialog", async () => {
	const { dialog } = await import("./testHarness");
	return dialog;
});

function authoritative(): ConfigOptionView[] {
	return [
		{
			id: "model",
			name: "Model",
			category: "model",
			currentValue: "real-model",
			options: [
				{ value: "real-model", name: "Real Model" },
				{ value: "other-model", name: "Other Model" },
			],
		},
		{
			id: "effort",
			name: "Effort",
			category: "thought_level",
			currentValue: "high",
			options: [
				{ value: "low", name: "Low" },
				{ value: "high", name: "High" },
			],
		},
	];
}

beforeEach(() => {
	vi.clearAllMocks();
	stubMatchMedia();
	api.getPrefs.mockResolvedValue({ recent: [{ path: "/repo" }] });
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.scopingTemplate.mockResolvedValue(null);
});

describe("pending pickers", () => {
	test("createPlan with empty options loads history and shows the plan pane", async () => {
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults(),
		});
		api.createPlan.mockResolvedValue({
			plans: [
				testEntry(),
				testEntry("2026-09-25.11-00-00", "scoping", "Untitled", false),
			],
			selected: { plan: "2026-09-25.11-00-00", role: "scoping" },
			config_defaults: testDefaults({ model: "stored-model", effort: null }),
		});
		const user = await openChat(api);
		await user.click(screen.getByRole("button", { name: "New plan" }));
		await vi.waitFor(() =>
			expect(api.loadHistory).toHaveBeenCalledWith({
				plan: "2026-09-25.11-00-00",
				role: "scoping",
			}),
		);
		await screen.findByText("No plan yet.");
		expect(screen.queryByText("TODOS")).not.toBeInTheDocument();
		expect(screen.queryByText("MODEL")).not.toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "Toggle settings" }));
		await screen.findByText("stored-model");
		expect(screen.getByText("MODEL")).toBeInTheDocument();
		expect(screen.getByLabelText("Model")).toBeDisabled();
	});

	test("incoming config_options enables authoritative lists", async () => {
		const executingPlan = "2026-09-25.10-54-59.slug";
		const executingKey = { plan: executingPlan, role: "executing" as const };
		api.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: [testEntry(executingPlan, "executing", "Shiny", true)],
			selected: executingKey,
			config_defaults: testDefaults({ model: "stale-model", effort: null }),
		});
		const user = await openChat(api);
		await user.click(screen.getByRole("button", { name: "Toggle settings" }));
		expect(screen.getByText("stale-model")).toBeInTheDocument();
		emitAppEvent(api, {
			type: "config_options",
			session: executingKey,
			options: authoritative(),
		});
		await screen.findByText("Real Model");
		expect(screen.queryByText("stale-model")).not.toBeInTheDocument();
		expect(screen.getByLabelText("Model")).not.toBeDisabled();
	});
});
