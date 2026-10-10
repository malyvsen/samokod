import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { CommitsPanel, SidePanel } from "./components/SidePanel";
import { testDefaults, testEntry, testEntryWith, testStatus } from "./fixtures";
import { api, openChat, stubMatchMedia } from "./testHarness";
import type { TodoView } from "./types";

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
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "main" });
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "main",
		plans: [testEntry()],
		selected: { plan: "2026-09-25.10-54-59", role: "scoping" },
		config_defaults: testDefaults(),
		scopingPreview: null,
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.planMdText.mockResolvedValue(null);
	api.selectPlan.mockResolvedValue({
		plans: [testEntry()],
		selected: { plan: "2026-09-25.10-54-59", role: "scoping" },
		config_defaults: testDefaults(),
		scopingPreview: null,
	});
});

const TODOS: TodoView[] = [
	{ content: "Add retry", status: "completed" },
	{ content: "Wire view", status: "in_progress" },
	{ content: "Verify green", status: "pending" },
];

function panel(todos: TodoView[] = []) {
	return render(<SidePanel todos={todos} />);
}

describe("side panel", () => {
	test("empty state shows no todos and no selectors", () => {
		panel();
		expect(screen.getByText("No todos yet")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).toBeInTheDocument();
		expect(screen.queryByText("MODEL")).not.toBeInTheDocument();
		expect(screen.queryByText("EFFORT")).not.toBeInTheDocument();
	});

	test("live list shows count and rows", () => {
		panel(TODOS);
		expect(screen.getByText("1/3")).toBeInTheDocument();
		expect(screen.getByText("Add retry")).toBeInTheDocument();
		expect(screen.getByText("Wire view")).toBeInTheDocument();
		expect(screen.getByText("Verify green")).toBeInTheDocument();
	});
});

describe("commits panel", () => {
	const COMMITS = ["abc plan work", "def more work"];

	test("empty state shows no commits", () => {
		render(<CommitsPanel commits={[]} />);
		expect(screen.getByText("No commits yet")).toBeInTheDocument();
		expect(screen.queryByText("COMMITS")).toBeInTheDocument();
	});

	test("live list shows count and rows", () => {
		const { container } = render(<CommitsPanel commits={COMMITS} />);
		expect(screen.getByText("COMMITS")).toBeInTheDocument();
		expect(screen.getByText("2")).toBeInTheDocument();
		expect(screen.getByText("abc plan work")).toBeInTheDocument();
		expect(screen.getByText("def more work")).toBeInTheDocument();
		const rows = container.querySelectorAll(".side .row[data-full]");
		expect(rows.length).toBe(2);
		expect(rows[0]?.getAttribute("data-full")).toBe("abc plan work");
	});
});

describe("landing sidebar", () => {
	test("selected landing shows commits while executing shows todos", async () => {
		const landing = testEntryWith(
			"2026-09-25.10-54-59.landing",
			"landing",
			"Shiny",
			true,
			[
				testStatus("scoping"),
				testStatus("executing"),
				testStatus("evergreening"),
				testStatus("landing"),
			],
			false,
			true,
			["abc plan work", "def more work"],
			false,
		);
		const executing = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
		);
		const plans = [landing, executing];
		api.selectPlan.mockResolvedValue({
			plans,
			selected: { plan: executing.name, role: "executing" },
			config_defaults: testDefaults(),
			scopingPreview: null,
		});
		const user = await openChat(api, {
			plans,
			selected: { plan: landing.name, role: "landing" },
			scopingPreview: null,
		});
		await screen.findByText("COMMITS");
		expect(screen.getByText("abc plan work")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).not.toBeInTheDocument();
		await user.click(
			screen.getByRole("button", {
				name: "Shiny, executing, opens Executing",
			}),
		);
		await screen.findByText("TODOS");
		expect(screen.queryByText("COMMITS")).not.toBeInTheDocument();
		expect(screen.queryByText("abc plan work")).not.toBeInTheDocument();
	});
});
