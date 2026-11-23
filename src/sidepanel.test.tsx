import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { TodoView } from "./bindings";
import { CommitsPanel, SidePanel } from "./components/SidePanel";
import { testDefaults, testEntry, testEntryWith, testStatus } from "./fixtures";
import { api, openChat, stubMatchMedia } from "./testHarness";

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
		scoping_preview: null,
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
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
		render(<CommitsPanel commits={[]} done={0} total={0} working={false} />);
		expect(screen.getByText("No commits yet")).toBeInTheDocument();
		expect(screen.queryByText("COMMITS")).toBeInTheDocument();
		expect(screen.queryByText("0/0")).not.toBeInTheDocument();
	});

	test("partial shows done/total with subjects only", () => {
		const { container } = render(
			<CommitsPanel commits={COMMITS} done={1} total={2} working={true} />,
		);
		expect(screen.getByText("COMMITS")).toBeInTheDocument();
		expect(screen.getByText("1/2")).toBeInTheDocument();
		expect(screen.getByText("[x]")).toBeInTheDocument();
		expect(screen.getByText("[>]")).toBeInTheDocument();
		expect(screen.getByText("plan work")).toBeInTheDocument();
		expect(screen.getByText("more work")).toBeInTheDocument();
		expect(screen.queryByText("abc plan work")).not.toBeInTheDocument();
		expect(screen.queryByText("def more work")).not.toBeInTheDocument();
		const rows = container.querySelectorAll(".side .row[data-full]");
		expect(rows.length).toBe(2);
		expect(rows[0]?.getAttribute("data-full")).toBe("plan work");
		expect(rows[1]?.getAttribute("data-full")).toBe("more work");
	});

	test("ready shows all done", () => {
		render(
			<CommitsPanel commits={COMMITS} done={2} total={2} working={false} />,
		);
		expect(screen.getByText("2/2")).toBeInTheDocument();
		expect(screen.getAllByText("[x]").length).toBe(2);
		expect(screen.queryByText("[>]")).not.toBeInTheDocument();
		expect(screen.queryByText("[ ]")).not.toBeInTheDocument();
	});

	test("blocked shows all pending", () => {
		render(
			<CommitsPanel commits={COMMITS} done={0} total={2} working={false} />,
		);
		expect(screen.getByText("0/2")).toBeInTheDocument();
		expect(screen.getAllByText("[ ]").length).toBe(2);
		expect(screen.queryByText("[x]")).not.toBeInTheDocument();
		expect(screen.queryByText("[>]")).not.toBeInTheDocument();
	});
});

describe("rebasing sidebar", () => {
	test("selected rebasing shows commits while executing shows todos", async () => {
		const rebasing = testEntryWith(
			"2026-09-25.10-54-59.rebasing",
			"rebasing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing"), testStatus("rebasing")],
			false,
			true,
			["abc plan work", "def more work"],
		);
		const executing = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
		);
		const plans = [rebasing, executing];
		api.selectPlan.mockResolvedValue({
			plans,
			selected: { plan: executing.name, role: "executing" },
			config_defaults: testDefaults(),
			scoping_preview: null,
		});
		const user = await openChat(api, {
			plans,
			selected: { plan: rebasing.name, role: "rebasing" },
			scoping_preview: null,
		});
		await screen.findByText("COMMITS");
		expect(screen.getByText("plan work")).toBeInTheDocument();
		expect(screen.getByText("more work")).toBeInTheDocument();
		expect(screen.queryByText("abc plan work")).not.toBeInTheDocument();
		expect(screen.queryByText("TODOS")).not.toBeInTheDocument();
		await user.click(
			screen.getByRole("button", {
				name: "Shiny, executing, opens Executing",
			}),
		);
		await screen.findByText("TODOS");
		expect(screen.queryByText("COMMITS")).not.toBeInTheDocument();
		expect(screen.queryByText("plan work")).not.toBeInTheDocument();
	});
});
