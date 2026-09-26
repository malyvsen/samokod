import { readFileSync } from "node:fs";
import { render, screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { PlansPanel } from "./components/PlansPanel";
import { testEntryWith, testStatus, testWorktree } from "./fixtures";
import type { PlanEntry } from "./types";

function panelProps(
	plans: PlanEntry[],
	selected: {
		plan: string;
		role: "scoping" | "executing" | "merging";
	} | null = null,
) {
	return {
		plans,
		selected,
		onSelect: vi.fn(),
		onNewPlan: vi.fn(),
		onExecute: vi.fn(),
		onAbandon: vi.fn(),
		onCancel: vi.fn(),
		onDone: vi.fn(),
		onBeginMerge: vi.fn(),
	};
}

function scopingPlan() {
	return testEntryWith(
		"2026-09-25.10-54-59",
		"scoping",
		"Parallel sessions",
		true,
		[testStatus("scoping")],
	);
}

function executingPlan() {
	return testEntryWith(
		"2026-09-25.10-54-59.slug",
		"executing",
		"Shiny",
		true,
		[testStatus("scoping"), testStatus("executing")],
		testWorktree(),
	);
}

function mergingPlan() {
	return testEntryWith(
		"2026-09-25.10-54-59.slug",
		"merging",
		"Shiny",
		true,
		[testStatus("scoping"), testStatus("executing"), testStatus("merging")],
		testWorktree(),
	);
}

describe("plans panel", () => {
	test("shows the new-plan button first", async () => {
		const props = panelProps([scopingPlan()]);
		const user = userEvent.setup();
		render(<PlansPanel {...props} />);
		await user.click(screen.getByRole("button", { name: "+ NEW PLAN" }));
		expect(props.onNewPlan).toHaveBeenCalledTimes(1);
	});

	test("derives the plan name from the title with an Untitled fallback", () => {
		render(
			<PlansPanel
				{...panelProps([
					testEntryWith("a", "scoping", "Shiny", true, [testStatus("scoping")]),
					testEntryWith("b", "scoping", "Untitled", false, [
						testStatus("scoping"),
					]),
				])}
			/>,
		);
		expect(screen.getByText("Shiny")).toBeInTheDocument();
		expect(screen.getByText("Untitled")).toBeInTheDocument();
	});

	test("an approved plan lists both its sessions", () => {
		render(<PlansPanel {...panelProps([executingPlan()])} />);
		expect(screen.getByText("Scoping")).toBeInTheDocument();
		expect(screen.getByText("Execution")).toBeInTheDocument();
	});

	test("dots follow working, approval, and failure flags", () => {
		const { container } = render(
			<PlansPanel
				{...panelProps([
					testEntryWith("run", "scoping", "Run", true, [
						testStatus("scoping", { working: true }),
					]),
					testEntryWith("wait", "scoping", "Wait", true, [
						testStatus("scoping", { approval: true }),
					]),
					testEntryWith("broke", "scoping", "Broke", true, [
						testStatus("scoping", { failed: true }),
					]),
					testEntryWith("idle", "scoping", "Idle", true, [
						testStatus("scoping"),
					]),
				])}
			/>,
		);
		expect(container.querySelector(".dot.running")).not.toBeNull();
		expect(container.querySelector(".dot.approval")).not.toBeNull();
		expect(container.querySelector(".dot.failed")).not.toBeNull();
		expect(container.querySelector(".dot.input")).not.toBeNull();
	});

	test("history rows stay gray", () => {
		const { container } = render(
			<PlansPanel
				{...panelProps([
					executingPlan(),
					testEntryWith("done", "completed", "Done", true, [
						testStatus("scoping"),
						testStatus("executing"),
					]),
				])}
			/>,
		);
		const doneDots = container.querySelectorAll(".dot.done");
		expect(doneDots.length).toBeGreaterThanOrEqual(3);
	});

	test("scoping rows offer abandon and execute", () => {
		render(<PlansPanel {...panelProps([scopingPlan()])} />);
		const row = screen
			.getByRole("button", { name: "Parallel sessions Scoping" })
			.closest(".session");
		if (row === null) throw new Error("row missing");
		const buttons = within(row as HTMLElement);
		expect(
			buttons.getByRole("button", { name: "Abandon 2026-09-25.10-54-59" }),
		).toBeInTheDocument();
		expect(
			buttons.getByRole("button", {
				name: "Send 2026-09-25.10-54-59 to execution",
			}),
		).toBeInTheDocument();
	});

	test("execution rows offer cancel and done", () => {
		render(<PlansPanel {...panelProps([executingPlan()])} />);
		const row = screen
			.getByRole("button", { name: "Shiny Execution" })
			.closest(".session");
		if (row === null) throw new Error("row missing");
		const buttons = within(row as HTMLElement);
		expect(
			buttons.getByRole("button", {
				name: "Cancel 2026-09-25.10-54-59.slug and delete branch",
			}),
		).toBeInTheDocument();
		expect(
			buttons.getByRole("button", {
				name: "Merge 2026-09-25.10-54-59.slug to main",
			}),
		).toBeInTheDocument();
	});

	test("diverged execution rows offer rebase instead of done", () => {
		const diverged = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
			testWorktree({ ffable: false }),
		);
		render(<PlansPanel {...panelProps([diverged])} />);
		expect(
			screen.getByRole("button", {
				name: "Rebase 2026-09-25.10-54-59.slug onto latest main",
			}),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", {
				name: "Merge 2026-09-25.10-54-59.slug to main",
			}),
		).toBeNull();
	});

	test("dirty execution rows disable merging with a tooltip", () => {
		const dirty = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
			testWorktree({ dirty: true }),
		);
		render(<PlansPanel {...panelProps([dirty])} />);
		const merge = screen.getByRole("button", {
			name: "Merge 2026-09-25.10-54-59.slug to main",
		});
		expect(merge).toBeDisabled();
		expect(merge.getAttribute("title")).toBe(
			"Commit or discard worktree changes first",
		);
	});

	test("merging plans list three sessions with cancel and finish", () => {
		render(<PlansPanel {...panelProps([mergingPlan()])} />);
		expect(screen.getByText("Scoping")).toBeInTheDocument();
		expect(screen.getByText("Execution")).toBeInTheDocument();
		expect(screen.getByText("Merging")).toBeInTheDocument();
		const row = screen
			.getByRole("button", { name: "Shiny Merging" })
			.closest(".session");
		if (row === null) throw new Error("row missing");
		const buttons = within(row as HTMLElement);
		expect(
			buttons.getByRole("button", { name: "Cancel merge and delete branch" }),
		).toBeInTheDocument();
		expect(
			buttons.getByRole("button", {
				name: "Finish 2026-09-25.10-54-59.slug merge",
			}),
		).toBeInTheDocument();
	});

	test("history and finished rows have no buttons", () => {
		render(
			<PlansPanel
				{...panelProps([
					executingPlan(),
					testEntryWith("done", "completed", "Done", true, [
						testStatus("scoping"),
						testStatus("executing"),
					]),
					testEntryWith("drop", "cancelled", "Drop", false, [
						testStatus("scoping"),
					]),
				])}
			/>,
		);
		const history = screen
			.getByRole("button", { name: "Shiny Scoping" })
			.closest(".session");
		const done = screen
			.getByRole("button", { name: "Done Execution" })
			.closest(".session");
		const dropped = screen
			.getByRole("button", { name: "Drop Scoping" })
			.closest(".session");
		for (const row of [history, done, dropped]) {
			if (row === null) throw new Error("row missing");
			expect(
				within(row as HTMLElement).queryByRole("button", {
					name: /Abandon|Cancel|Send|Mark/,
				}),
			).toBeNull();
		}
	});

	test("execute stays disabled without plan.md", () => {
		render(
			<PlansPanel
				{...panelProps([
					testEntryWith("bare", "scoping", "Bare", false, [
						testStatus("scoping"),
					]),
				])}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Send bare to execution" }),
		).toBeDisabled();
	});

	test("row actions call back with the session key", async () => {
		const props = panelProps([scopingPlan()]);
		const user = userEvent.setup();
		render(<PlansPanel {...props} />);
		await user.click(
			screen.getByRole("button", {
				name: "Send 2026-09-25.10-54-59 to execution",
			}),
		);
		expect(props.onExecute).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59",
			role: "scoping",
		});
		await user.click(
			screen.getByRole("button", { name: "Abandon 2026-09-25.10-54-59" }),
		);
		expect(props.onAbandon).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59",
			role: "scoping",
		});
	});

	test("row actions never act on the wrong session", async () => {
		const props = panelProps([scopingPlan(), executingPlan()]);
		const user = userEvent.setup();
		render(<PlansPanel {...props} />);
		await user.click(
			screen.getByRole("button", {
				name: "Merge 2026-09-25.10-54-59.slug to main",
			}),
		);
		expect(props.onDone).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59.slug",
			role: "executing",
		});
		expect(props.onAbandon).not.toHaveBeenCalled();
	});

	test("rebase actions call back with the executing session", async () => {
		const diverged = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
			testWorktree({ ffable: false }),
		);
		const props = panelProps([diverged]);
		const user = userEvent.setup();
		render(<PlansPanel {...props} />);
		await user.click(
			screen.getByRole("button", {
				name: "Rebase 2026-09-25.10-54-59.slug onto latest main",
			}),
		);
		expect(props.onBeginMerge).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59.slug",
			role: "executing",
		});
		expect(props.onDone).not.toHaveBeenCalled();
	});

	test("selecting a row marks it selected", async () => {
		const props = panelProps([scopingPlan(), executingPlan()], {
			plan: "2026-09-25.10-54-59",
			role: "scoping",
		});
		const user = userEvent.setup();
		render(<PlansPanel {...props} />);
		await user.click(screen.getByRole("button", { name: "Shiny Execution" }));
		expect(props.onSelect).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59.slug",
			role: "executing",
		});
	});

	test("action buttons carry no tooltips", () => {
		const { container } = render(
			<PlansPanel {...panelProps([scopingPlan(), executingPlan()])} />,
		);
		for (const button of container.querySelectorAll(".sbtn")) {
			expect(button.getAttribute("title")).toBeNull();
			expect(
				button.textContent === "✕" ||
					button.textContent === ">" ||
					button.textContent === "✓",
			).toBe(true);
		}
	});

	test("the sharp-corners reset leaves dots circular in CSS", () => {
		const css = readFileSync("src/App.css", "utf8");
		expect(css).not.toContain(".app *");
		const reset = /\.app[^{]*\{[^}]*border-radius[^}]*\}/.exec(css)?.[0] ?? "";
		expect(reset).toContain(".dot");
	});

	test("the select button fills the whole row in CSS", () => {
		const css = readFileSync("src/App.css", "utf8");
		const row = /\.session\s+\.srow\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(row).toContain("align-self: stretch");
	});

	test("action buttons stay hover-only in CSS", () => {
		const css = readFileSync("src/App.css", "utf8");
		const hidden = /\.plans\s+\.sbtn\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(hidden).toContain("opacity: 0");
		const shown =
			/\.plans\s+\.session:hover\s+\.sbtn[^{]*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(shown).toContain("opacity: 1");
	});
});
