import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry } from "./fixtures";
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

function modelOption(currentValue: string): ConfigOptionView {
	return {
		id: "model",
		name: "Model",
		category: "model",
		currentValue,
		options: [
			{ value: "openai/gpt-5", name: "GPT-5" },
			{ value: "openai/gpt-4o", name: "GPT-4o" },
		],
	};
}

function modeOption(): ConfigOptionView {
	return {
		id: "mode",
		name: "Session Mode",
		category: "mode",
		currentValue: "build",
		options: [{ value: "build", name: "build" }],
	};
}

function effortOption(currentValue: string): ConfigOptionView {
	return {
		id: "effort",
		name: "Effort",
		category: "thought_level",
		currentValue,
		options: [
			{ value: "low", name: "Low" },
			{ value: "medium", name: "Medium" },
			{ value: "high", name: "High" },
		],
	};
}

function deferred<T>() {
	let resolve!: (value: T) => void;
	let reject!: (reason?: unknown) => void;
	const promise = new Promise<T>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

beforeEach(() => {
	stubMatchMedia();
	api.getPrefs.mockResolvedValue({ recent: [] });
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	api.setGlobalConfigOption.mockReset();
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.scopingTemplate.mockResolvedValue(null);
	api.planMdText.mockResolvedValue(null);
});

async function openChatWith(options: ConfigOptionView[]) {
	api.getPrefs.mockResolvedValue({
		recent: [{ path: "/repo" }],
	});
	const executingPlan = "2026-09-25.10-54-59.slug";
	const executingKey = { plan: executingPlan, role: "executing" as const };
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "feature",
		plans: [testEntry(executingPlan, "executing", "Shiny", true)],
		selected: executingKey,
		config_defaults: testDefaults(),
	});
	const user = await openChat(api);
	emitAppEvent(api, {
		type: "config_options",
		session: executingKey,
		options,
	});
	await user.click(screen.getByRole("button", { name: "Toggle settings" }));
	return { user, key: executingKey };
}

describe("config change", () => {
	test("adopts the returned list including new options", async () => {
		const { user } = await openChatWith([
			modelOption("openai/gpt-4o"),
			modeOption(),
		]);
		api.setGlobalConfigOption.mockResolvedValue([
			modelOption("openai/gpt-5"),
			effortOption("low"),
			modeOption(),
		]);
		await user.click(screen.getByLabelText("Model"));
		await user.click(screen.getByText("GPT-5"));
		expect(api.setGlobalConfigOption).toHaveBeenCalledWith(
			"model",
			"openai/gpt-5",
		);
		await screen.findByLabelText("Effort");
		expect(screen.getByText("GPT-5")).toBeInTheDocument();
	});

	test("ignores a stale earlier response", async () => {
		const { user } = await openChatWith([
			modelOption("openai/gpt-5"),
			effortOption("low"),
			modeOption(),
		]);
		const first = deferred<ConfigOptionView[]>();
		const second = deferred<ConfigOptionView[]>();
		api.setGlobalConfigOption
			.mockReturnValueOnce(first.promise)
			.mockReturnValueOnce(second.promise);
		await user.click(screen.getByLabelText("Effort"));
		await user.click(screen.getByText("High"));
		await user.click(screen.getByLabelText("Model"));
		await user.click(screen.getByText("GPT-4o"));
		first.resolve([
			modelOption("openai/gpt-5"),
			effortOption("high"),
			modeOption(),
		]);
		second.resolve([modelOption("openai/gpt-4o"), modeOption()]);
		await screen.findByText("GPT-4o");
		expect(screen.queryByLabelText("Effort")).not.toBeInTheDocument();
	});

	test("keeps the previous list when the change fails", async () => {
		const { user } = await openChatWith([
			modelOption("openai/gpt-5"),
			effortOption("low"),
			modeOption(),
		]);
		api.setGlobalConfigOption.mockRejectedValue(new Error("denied"));
		await user.click(screen.getByLabelText("Effort"));
		await user.click(screen.getByText("Medium"));
		await screen.findByText("Low");
		expect(screen.getByText("GPT-5")).toBeInTheDocument();
	});
});
