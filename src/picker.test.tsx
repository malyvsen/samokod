import { render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import type { RecentRepo } from "./bindings";
import { RepoPicker } from "./components/RepoPicker";
import { testDefaults, testEntry, testKey } from "./fixtures";
import { api, dialog, stubMatchMedia } from "./testHarness";

vi.mock("./api", async () => {
	const { api } = await import("./testHarness");
	return api;
});

vi.mock("@tauri-apps/plugin-dialog", async () => {
	const { dialog } = await import("./testHarness");
	return dialog;
});

function pickerProps(overrides: {
	recent?: RecentRepo[];
	error?: string | null;
	openPaths?: Set<string>;
	onOpen?: (path: string) => void;
}) {
	return {
		title: "SAMOKOD",
		subtitle: "open a git repository to start planning",
		recent: overrides.recent ?? [
			{ path: "~/code/samokod" },
			{ path: "~/code/dotfiles" },
		],
		error: overrides.error ?? null,
		openPaths: overrides.openPaths ?? new Set<string>(),
		onOpen: overrides.onOpen ?? vi.fn(),
		onBrowse: vi.fn(),
		onDismissError: vi.fn(),
	};
}

describe("repo picker", () => {
	test("lists recent repos with open actions", () => {
		render(<RepoPicker {...pickerProps({})} />);
		expect(screen.getByText("~/code/samokod")).toBeInTheDocument();
		expect(screen.getAllByText("open")).toHaveLength(2);
		expect(screen.getByText("browse…")).toBeInTheDocument();
	});

	test("shows inline error for non-git picks", () => {
		render(
			<RepoPicker
				{...pickerProps({ error: "~/downloads/notes - not a git repository" })}
			/>,
		);
		expect(screen.getByRole("alert")).toHaveTextContent("not a git repository");
	});

	test("marks repos open elsewhere with no action", () => {
		render(
			<RepoPicker
				{...pickerProps({
					openPaths: new Set(["~/code/samokod"]),
				})}
			/>,
		);
		expect(screen.getByText("OPEN ELSEWHERE")).toBeInTheDocument();
		expect(screen.getAllByRole("button", { name: "open" })).toHaveLength(1);
	});

	test("free repos show an open action", async () => {
		const onOpen = vi.fn();
		const user = userEvent.setup();
		render(
			<RepoPicker
				{...pickerProps({
					openPaths: new Set(["~/code/samokod"]),
					onOpen,
				})}
			/>,
		);
		await user.click(screen.getByRole("button", { name: "open" }));
		expect(onOpen).toHaveBeenCalledWith("~/code/dotfiles");
		expect(onOpen).toHaveBeenCalledTimes(1);
	});
});

describe("picker already-open conflict", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		stubMatchMedia();
		api.getPrefs.mockResolvedValue({
			recent: [{ path: "/repo-taken" }, { path: "/repo-free" }],
		});
		api.validateRepo.mockImplementation(async (path: string) => ({
			root: path,
			branch: "main",
		}));
		api.listOpenRepos.mockResolvedValue([
			{ path: "/repo-taken", window: "window-1" },
		]);
		api.openRepo.mockRejectedValue(new Error("already open: /repo-taken"));
		api.takePendingRepo.mockResolvedValue(null);
		dialog.open.mockResolvedValue("/repo-taken");
	});

	test("browse conflict shows a notice and stays on the picker", async () => {
		const user = userEvent.setup();
		render(<App />);
		await user.click(await screen.findByRole("button", { name: "browse…" }));
		expect(await screen.findByRole("alert")).toHaveTextContent(
			"/repo-taken is already open - switched to that window",
		);
		expect(screen.getByRole("button", { name: "browse…" })).toBeInTheDocument();
		expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
	});
});

describe("cli pending path", () => {
	beforeEach(() => {
		vi.clearAllMocks();
		stubMatchMedia();
		api.getPrefs.mockResolvedValue({ recent: [] });
		api.listOpenRepos.mockResolvedValue([]);
		api.validateRepo.mockImplementation(async (path: string) => ({
			root: path,
			branch: "main",
		}));
		api.takePendingRepo.mockResolvedValue(null);
		api.warmSession.mockResolvedValue(undefined);
		api.loadHistory.mockResolvedValue(undefined);
		api.planMdText.mockResolvedValue(null);
	});

	test("no pending path stays on the picker", async () => {
		render(<App />);
		expect(
			await screen.findByRole("button", { name: "browse…" }),
		).toBeInTheDocument();
		expect(api.openRepo).not.toHaveBeenCalled();
		expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
	});

	test("pending path opens directly in chat", async () => {
		api.takePendingRepo.mockResolvedValue("/repo-cli");
		api.openRepo.mockResolvedValue({
			repo_root: "/repo-cli",
			branch: "main",
			plans: [testEntry()],
			selected: testKey(),
			config_defaults: testDefaults(),
			scoping_preview: "TEMPLATE",
		});
		render(<App />);
		await screen.findByRole("textbox", { name: "Ask for a change…" });
		expect(api.validateRepo).toHaveBeenCalledWith("/repo-cli");
		expect(api.openRepo).toHaveBeenCalledWith("/repo-cli");
		expect(
			screen.queryByRole("button", { name: "browse…" }),
		).not.toBeInTheDocument();
	});
});
