import { act, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry, testKey } from "./fixtures";
import { clearPreviewCache } from "./sessions/preview";
import { api, emitAppEvents, openChat, stubMatchMedia } from "./testHarness";
import type { AppEvent } from "./types";

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
});

describe("rapid chat events", () => {
	test("three agent_text chunks in one act concatenate", async () => {
		await openChat(api);
		emitAppEvents(api, [
			{ type: "agent_text", session: testKey(), chunk: "Ever" },
			{ type: "agent_text", session: testKey(), chunk: "greened the " },
			{ type: "agent_text", session: testKey(), chunk: "title-only" },
		]);
		expect(screen.getByText("Evergreened the title-only")).toBeInTheDocument();
	});

	test("optimistic user bubble survives a racing agent chunk", async () => {
		const user = await openChat(api);
		type Handler = (event: AppEvent) => void;
		const calls = api.onAppEvent.mock.calls as unknown as Handler[][];
		const handler = calls[0]?.[0];
		if (handler === undefined) throw new Error("no app event handler");
		await act(async () => {
			const typing = user.keyboard("hello{Enter}");
			await typing;
			await Promise.resolve();
			await Promise.resolve();
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
