import { act, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry, testKey } from "./fixtures";
import {
	api,
	appEventHandler,
	emitAppEvents,
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
		scopingPreview: null,
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.sendPrompt.mockResolvedValue(undefined);
});

describe("streaming", () => {
	test("concatenates chunks arriving before render", async () => {
		await openChat(api);
		emitAppEvents(api, [
			{ type: "agent_text", session: testKey(), chunk: "Hello " },
			{ type: "agent_text", session: testKey(), chunk: "brave " },
			{ type: "agent_text", session: testKey(), chunk: "world" },
		]);
		expect(screen.getByText("Hello brave world")).toBeInTheDocument();
	});

	test("keeps the optimistic user message when an agent chunk arrives before render", async () => {
		const user = await openChat(api);
		const handler = appEventHandler(api);
		await act(async () => {
			await user.keyboard("hello{Enter}");
			handler({
				type: "agent_text",
				session: testKey(),
				chunk: "agent reply",
			});
		});
		expect(screen.getByText("hello")).toBeInTheDocument();
		expect(screen.getByText("agent reply")).toBeInTheDocument();
	});
});
