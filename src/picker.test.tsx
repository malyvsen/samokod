import { render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import type { RecentRepo } from "./bindings";
import { RepoPicker } from "./components/RepoPicker";
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
	onFocusRepo?: (path: string) => void;
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
		onOpen: vi.fn(),
		onFocusRepo: overrides.onFocusRepo ?? vi.fn(),
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

	test("marks repos open elsewhere with a switch action", () => {
		const onFocusRepo = vi.fn();
		render(
			<RepoPicker
				{...pickerProps({
					openPaths: new Set(["~/code/samokod"]),
					onFocusRepo,
				})}
			/>,
		);
		expect(screen.getByText("OPEN ELSEWHERE")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "switch" })).toBeInTheDocument();
		expect(screen.getAllByText("open")).toHaveLength(1);
	});

	test("switch focuses the owning window", async () => {
		const onFocusRepo = vi.fn();
		const user = userEvent.setup();
		render(
			<RepoPicker
				{...pickerProps({
					openPaths: new Set(["~/code/samokod"]),
					onFocusRepo,
				})}
			/>,
		);
		await user.click(screen.getByRole("button", { name: "switch" }));
		expect(onFocusRepo).toHaveBeenCalledWith("~/code/samokod");
		expect(onFocusRepo).toHaveBeenCalledTimes(1);
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
		api.focusWindow.mockResolvedValue(undefined);
		api.openRepo.mockRejectedValue(new Error("already open: /repo-taken"));
		dialog.open.mockResolvedValue("/repo-taken");
	});

	test("browse conflict shows a notice and stays on the picker", async () => {
		const user = userEvent.setup();
		render(<App />);
		await user.click(await screen.findByRole("button", { name: "browse…" }));
		// The backend focuses the owning window itself; the picker only
		// explains why nothing opened here.
		expect(api.focusWindow).not.toHaveBeenCalled();
		expect(await screen.findByRole("alert")).toHaveTextContent(
			"/repo-taken is already open - switched to that window",
		);
		expect(screen.getByRole("button", { name: "browse…" })).toBeInTheDocument();
		expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
	});
});
