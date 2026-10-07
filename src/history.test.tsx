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

describe("history", () => {
	test("unknown session shows opening copy", async () => {
		await openChat(api);
		expect(screen.getByText(/opening session/)).toBeInTheDocument();
		expect(screen.getByText("getting session ready")).toBeInTheDocument();
	});

	test("preparing shows fresh session without replay claim", async () => {
		await openChat(api);
		expect(api.loadHistory).toHaveBeenCalledWith(testKey());
		emitAppEvent(api, { type: "history_preparing", session: testKey() });
		expect(screen.getByText("preparing session")).toBeInTheDocument();
		expect(
			screen.queryByText("replaying past messages"),
		).not.toBeInTheDocument();
	});

	test("loading hint shows while history streams", async () => {
		await openChat(api);
		emitAppEvent(api, { type: "history_preparing", session: testKey() });
		emitAppEvent(api, { type: "history_begin", session: testKey() });
		expect(screen.getByText("replaying past messages")).toBeInTheDocument();
		emitAppEvent(api, { type: "history_done", session: testKey() });
		expect(
			screen.queryByText("replaying past messages"),
		).not.toBeInTheDocument();
	});

	test("failed history shows retry and retry reloads", async () => {
		const user = await openChat(api);
		emitAppEvent(api, {
			type: "history_failed",
			session: testKey(),
			raw: "boom",
			hint: "history hint",
			retryable: true,
		});
		expect(screen.getByText("history hint")).toBeInTheDocument();
		expect(screen.getByText(/boom/)).toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "retry" }));
		expect(api.loadHistory).toHaveBeenCalledTimes(2);
	});

	test("non-retryable history hides retry", async () => {
		await openChat(api);
		emitAppEvent(api, {
			type: "history_failed",
			session: testKey(),
			raw: "no saved session found for this plan - it predates session recording or its session was pruned",
			hint: "this plan's history is unavailable - it predates session recording or was pruned",
			retryable: false,
		});
		expect(
			screen.getByText(
				"this plan's history is unavailable - it predates session recording or was pruned",
			),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});

	test("restored history renders in the transcript", async () => {
		await openChat(api);
		emitAppEvent(api, { type: "history_preparing", session: testKey() });
		emitAppEvent(api, { type: "history_begin", session: testKey() });
		emitAppEvent(api, {
			type: "user_text",
			session: testKey(),
			chunk: "hello",
		});
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "hi there",
		});
		emitAppEvent(api, { type: "history_done", session: testKey() });
		expect(screen.getByText("hello")).toBeInTheDocument();
		expect(screen.getByText("hi there")).toBeInTheDocument();
	});
});
