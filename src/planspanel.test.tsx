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

	test("renders all five sections in fixed order", () => {
		render(
			<PlansPanel
				{...panelProps([executingPlan(), scopingPlan(), mergingPlan()])}
			/>,
		);
		const headers = screen
			.getAllByText(/^(SCOPING|EXECUTING|MERGING|COMPLETED|CANCELLED)$/)
			.map((node) => node.textContent);
		expect(headers).toEqual([
			"SCOPING",
			"EXECUTING",
			"MERGING",
			"COMPLETED",
			"CANCELLED",
		]);
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

	test("collapsed plans expose only their header", () => {
		render(<PlansPanel {...panelProps([executingPlan()], null)} />);
		expect(screen.getByText("Shiny")).toBeInTheDocument();
		expect(screen.queryByText("Scoping")).toBeNull();
		expect(screen.queryByText("Executing")).toBeNull();
	});

	test("an approved plan lists both its sessions once expanded", () => {
		render(
			<PlansPanel
				{...panelProps([executingPlan()], {
					plan: "2026-09-25.10-54-59.slug",
					role: "executing",
				})}
			/>,
		);
		expect(screen.getByText("Scoping")).toBeInTheDocument();
		expect(screen.getByText("Executing")).toBeInTheDocument();
	});

	test("only plans waiting on the user show attention", () => {
		const { container } = render(
			<PlansPanel
				{...panelProps([
					testEntryWith("idle", "scoping", "Idle", true, [
						testStatus("scoping"),
					]),
					testEntryWith("wait", "scoping", "Wait", true, [
						testStatus("scoping", { approval: true }),
					]),
					testEntryWith("broke", "scoping", "Broke", true, [
						testStatus("scoping", { failed: true }),
					]),
					testEntryWith("run", "scoping", "Run", true, [
						testStatus("scoping", { working: true }),
					]),
					testEntryWith("done", "completed", "Done", true, [
						testStatus("scoping"),
						testStatus("executing"),
					]),
				])}
			/>,
		);
		expect(container.querySelector(".adot.idle")).not.toBeNull();
		expect(container.querySelector(".adot.approval")).not.toBeNull();
		expect(container.querySelector(".adot.failed")).not.toBeNull();
		expect(container.querySelectorAll(".adot").length).toBe(3);
	});

	test("attention dots name their cause", () => {
		const { container } = render(
			<PlansPanel
				{...panelProps([
					testEntryWith("idle", "scoping", "Idle", true, [
						testStatus("scoping"),
					]),
					testEntryWith("wait", "scoping", "Wait", true, [
						testStatus("scoping", { approval: true }),
					]),
					testEntryWith("broke", "scoping", "Broke", true, [
						testStatus("scoping", { failed: true }),
					]),
				])}
			/>,
		);
		const tips = [...container.querySelectorAll(".adot")].map((node) =>
			node.getAttribute("title"),
		);
		expect(tips).toEqual([
			"needs input",
			"needs approval",
			"failed, needs a response",
		]);
	});

	test("headers name the title, phase, and target agent", () => {
		render(<PlansPanel {...panelProps([executingPlan()], null)} />);
		expect(
			screen.getByRole("button", {
				name: "Shiny, executing, opens Executing",
			}),
		).toBeInTheDocument();
	});

	test("header clicks select the latest role", async () => {
		const user = userEvent.setup();
		for (const [plan, role] of [
			[scopingPlan(), "scoping"],
			[executingPlan(), "executing"],
			[mergingPlan(), "merging"],
		] as const) {
			const props = panelProps([plan], null);
			const { unmount } = render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", {
					name: new RegExp(`opens ${role}$`, "i"),
				}),
			);
			expect(props.onSelect).toHaveBeenCalledWith({
				plan: plan.name,
				role,
			});
			unmount();
		}
	});

	test("scoping rows offer cancel and execute", () => {
		render(
			<PlansPanel
				{...panelProps([scopingPlan()], {
					plan: "2026-09-25.10-54-59",
					role: "scoping",
				})}
			/>,
		);
		const row = screen
			.getByRole("button", { name: "Parallel sessions Scoping" })
			.closest(".session");
		if (row === null) throw new Error("row missing");
		const buttons = within(row as HTMLElement);
		expect(
			buttons.getByRole("button", { name: "Cancel 2026-09-25.10-54-59" }),
		).toBeInTheDocument();
		expect(
			buttons.getByRole("button", {
				name: "Send 2026-09-25.10-54-59 to execution",
			}),
		).toBeInTheDocument();
	});

	test("executing rows offer cancel and done", () => {
		render(
			<PlansPanel
				{...panelProps([executingPlan()], {
					plan: "2026-09-25.10-54-59.slug",
					role: "executing",
				})}
			/>,
		);
		const row = screen
			.getByRole("button", { name: "Shiny Executing" })
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

	test("diverged executing rows offer rebase instead of done", () => {
		const diverged = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
			testWorktree({ ffable: false }),
		);
		render(
			<PlansPanel
				{...panelProps([diverged], {
					plan: "2026-09-25.10-54-59.slug",
					role: "executing",
				})}
			/>,
		);
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

	test("dirty executing rows disable merging with a tooltip", () => {
		const dirty = testEntryWith(
			"2026-09-25.10-54-59.slug",
			"executing",
			"Shiny",
			true,
			[testStatus("scoping"), testStatus("executing")],
			testWorktree({ dirty: true }),
		);
		render(
			<PlansPanel
				{...panelProps([dirty], {
					plan: "2026-09-25.10-54-59.slug",
					role: "executing",
				})}
			/>,
		);
		const merge = screen.getByRole("button", {
			name: "Merge 2026-09-25.10-54-59.slug to main",
		});
		expect(merge).toBeDisabled();
		expect(merge.getAttribute("title")).toBe(
			"Commit or discard worktree changes first",
		);
	});

	test("merging plans list three sessions with cancel and finish", () => {
		render(
			<PlansPanel
				{...panelProps([mergingPlan()], {
					plan: "2026-09-25.10-54-59.slug",
					role: "merging",
				})}
			/>,
		);
		expect(screen.getByText("Scoping")).toBeInTheDocument();
		expect(screen.getByText("Executing")).toBeInTheDocument();
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

	test("buttons appear only on the expanded active agent", () => {
		const { container } = render(
			<PlansPanel
				{...panelProps([scopingPlan(), executingPlan()], {
					plan: "2026-09-25.10-54-59.slug",
					role: "executing",
				})}
			/>,
		);
		expect(container.querySelectorAll(".sbtn").length).toBeGreaterThan(0);
		const collapsed = screen
			.getByRole("button", {
				name: "Parallel sessions, scoping, opens Scoping",
			})
			.closest(".plan-group");
		if (collapsed === null) throw new Error("plan missing");
		expect(
			within(collapsed as HTMLElement).queryByRole("button", {
				name: /Cancel|Send|Merge/,
			}),
		).toBeNull();
	});

	test("inactive and finished rows have no buttons", () => {
		render(
			<PlansPanel
				{...panelProps(
					[
						executingPlan(),
						testEntryWith("done", "completed", "Done", true, [
							testStatus("scoping"),
							testStatus("executing"),
						]),
						testEntryWith("drop", "cancelled", "Drop", false, [
							testStatus("scoping"),
						]),
					],
					{ plan: "2026-09-25.10-54-59.slug", role: "executing" },
				)}
			/>,
		);
		const inactive = screen
			.getByRole("button", { name: "Shiny Scoping" })
			.closest(".session");
		if (inactive === null) throw new Error("row missing");
		expect(
			within(inactive as HTMLElement).queryByRole("button", {
				name: /Cancel|Send|Mark/,
			}),
		).toBeNull();
		const { container } = render(
			<PlansPanel
				{...panelProps(
					[
						testEntryWith("done", "completed", "Done", true, [
							testStatus("scoping"),
							testStatus("executing"),
						]),
					],
					{ plan: "done", role: "executing" },
				)}
			/>,
		);
		expect(container.querySelectorAll(".sbtn").length).toBe(0);
	});

	test("execute stays disabled without plan.md", () => {
		render(
			<PlansPanel
				{...panelProps(
					[
						testEntryWith("bare", "scoping", "Bare", false, [
							testStatus("scoping"),
						]),
					],
					{ plan: "bare", role: "scoping" },
				)}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Send bare to execution" }),
		).toBeDisabled();
	});

	test("row actions call back with the session key", async () => {
		const props = panelProps([scopingPlan()], {
			plan: "2026-09-25.10-54-59",
			role: "scoping",
		});
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
			screen.getByRole("button", { name: "Cancel 2026-09-25.10-54-59" }),
		);
		expect(props.onCancel).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59",
			role: "scoping",
		});
	});

	test("row actions never act on the wrong session", async () => {
		const props = panelProps([scopingPlan(), executingPlan()], {
			plan: "2026-09-25.10-54-59.slug",
			role: "executing",
		});
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
		expect(props.onCancel).not.toHaveBeenCalled();
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
		const props = panelProps([diverged], {
			plan: "2026-09-25.10-54-59.slug",
			role: "executing",
		});
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
			plan: "2026-09-25.10-54-59.slug",
			role: "scoping",
		});
		const user = userEvent.setup();
		render(<PlansPanel {...props} />);
		await user.click(screen.getByRole("button", { name: "Shiny Executing" }));
		expect(props.onSelect).toHaveBeenCalledWith({
			plan: "2026-09-25.10-54-59.slug",
			role: "executing",
		});
	});

	test("action buttons carry no tooltips", () => {
		const { container } = render(
			<PlansPanel
				{...panelProps([scopingPlan(), executingPlan()], {
					plan: "2026-09-25.10-54-59.slug",
					role: "executing",
				})}
			/>,
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

	test("expanded agents render muted in CSS", () => {
		const css = readFileSync("src/App.css", "utf8");
		const label = /\.session\s+\.slabel\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(label).toContain("#9d9a92");
	});

	test("the sharp-corners reset leaves attention dots circular in CSS", () => {
		const css = readFileSync("src/App.css", "utf8");
		expect(css).not.toContain(".app *");
		const reset = /\.app[^{]*\{[^}]*border-radius[^}]*\}/.exec(css)?.[0] ?? "";
		expect(reset).toContain(".adot");
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
