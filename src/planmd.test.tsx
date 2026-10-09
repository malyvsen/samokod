import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { PlanMdPane } from "./components/PlanMdPane";
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

function appCss() {
	return readFileSync("src/App.css", "utf8");
}

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
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.scopingTemplate.mockResolvedValue(null);
	api.planMdText.mockResolvedValue(null);
	api.selectPlan.mockResolvedValue({
		plans: [testEntry()],
		selected: testKey(),
		config_defaults: testDefaults(),
	});
});

describe("plan md pane", () => {
	test("empty state is exactly No plan yet", () => {
		render(<PlanMdPane text={null} />);
		expect(screen.getByText("No plan yet.")).toBeInTheDocument();
	});

	test("populated renders markdown", () => {
		render(<PlanMdPane text={"# Shiny\n\nSteps."} />);
		expect(screen.getByText("Shiny")).toBeInTheDocument();
		expect(screen.getByText("Steps.")).toBeInTheDocument();
		expect(screen.queryByText("No plan yet.")).not.toBeInTheDocument();
	});

	test("scoping selects text instead of editing", () => {
		const { container } = render(<PlanMdPane text={"# Shiny"} />);
		const pane = container.querySelector(".planmd") as HTMLElement;
		const css = appCss();
		expect(css).toContain("user-select: text");
		expect(pane).not.toBeNull();
	});
});

describe("scoping right pane", () => {
	test("empty scoping shows No plan yet without todos or selectors", async () => {
		api.planMdText.mockResolvedValue(null);
		await openChat(api);
		await screen.findByText("No plan yet.");
		expect(screen.queryByText("TODOS")).not.toBeInTheDocument();
		expect(screen.queryByText("MODEL")).not.toBeInTheDocument();
	});

	test("populated scoping renders the live plan", async () => {
		api.planMdText.mockResolvedValue("# Live plan\n\nWIP.");
		await openChat(api);
		await screen.findByText("Live plan");
		expect(screen.getByText("WIP.")).toBeInTheDocument();
		expect(screen.queryByText("TODOS")).not.toBeInTheDocument();
	});

	test("switching to executing shows todos instead of the plan", async () => {
		api.planMdText.mockResolvedValue("# Live plan");
		await openChat(api);
		await screen.findByText("Live plan");
		emitAppEvent(api, {
			type: "plans_changed",
			plans: [
				testEntry("2026-09-25.10-54-59.slug", "executing", "Shiny", true),
			],
			selected: { plan: "2026-09-25.10-54-59.slug", role: "executing" },
		});
		await screen.findByText("TODOS");
		expect(screen.queryByText("Live plan")).not.toBeInTheDocument();
		expect(screen.queryByText("No plan yet.")).not.toBeInTheDocument();
	});

	test("chat and plan share a 50-50 split", () => {
		const css = appCss();
		const chat = /\.chatcol\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(chat).toContain("flex: 1");
		const pane = /\.side\.planmd-pane\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(pane).toContain("flex: 1");
	});
});
