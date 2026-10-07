import { readFileSync } from "node:fs";
import { render, screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { PlansPanel } from "./components/PlansPanel";
import { testEntryWith, testStatus, testWorktree } from "./fixtures";
import type { PlanEntry } from "./types";

const SECTIONS = ["SCOPING", "EXECUTING", "LANDING", "COMPLETED", "CANCELLED"];

function panelProps(
	plans: PlanEntry[],
	selected: {
		plan: string;
		role: "scoping" | "executing" | "landing";
	} | null = null,
) {
	return {
		plans,
		selected,
		onSelect: vi.fn(),
		onNewPlan: vi.fn(),
		onCancel: vi.fn(),
		onSetMode: vi.fn(),
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

function landingPlan() {
	return testEntryWith(
		"2026-09-25.10-54-59.landing",
		"landing",
		"Shiny",
		true,
		[testStatus("scoping"), testStatus("executing"), testStatus("landing")],
		testWorktree(),
	);
}

function sectionHeaders() {
	return screen
		.getAllByText(/^(SCOPING|EXECUTING|LANDING|COMPLETED|CANCELLED)$/)
		.map((node) => node.textContent);
}

function planGroup(headerName: string) {
	const group = screen
		.getByRole("button", { name: headerName })
		.closest(".plan-group");
	if (group === null) throw new Error("plan missing");
	return group as HTMLElement;
}

function appCss() {
	return readFileSync("src/App.css", "utf8");
}

describe("plans panel", () => {
	describe("sections", () => {
		test("shows a hover-only plus next to scoping", async () => {
			const props = panelProps([scopingPlan()]);
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			expect(screen.queryByRole("button", { name: "+ NEW PLAN" })).toBeNull();
			const plus = screen.getByRole("button", { name: "New plan" });
			expect(plus.textContent).toBe("+");
			await user.click(plus);
			expect(props.onNewPlan).toHaveBeenCalledTimes(1);
		});

		test("renders all five sections in fixed order", () => {
			render(
				<PlansPanel
					{...panelProps([executingPlan(), scopingPlan(), landingPlan()])}
				/>,
			);
			expect(sectionHeaders()).toEqual(SECTIONS);
		});

		test("still renders sections without plans", () => {
			render(<PlansPanel {...panelProps([])} />);
			expect(sectionHeaders()).toEqual(SECTIONS);
		});
	});

	describe("headers", () => {
		test("shows titles with an Untitled fallback", () => {
			render(
				<PlansPanel
					{...panelProps([
						testEntryWith("a", "scoping", "Shiny", true, [
							testStatus("scoping"),
						]),
						testEntryWith("b", "scoping", "Untitled", false, [
							testStatus("scoping"),
						]),
					])}
				/>,
			);
			expect(screen.getByText("Shiny")).toBeInTheDocument();
			expect(screen.getByText("Untitled")).toBeInTheDocument();
		});

		test("names the title, phase, and target role", () => {
			render(<PlansPanel {...panelProps([executingPlan()], null)} />);
			expect(
				screen.getByRole("button", {
					name: "Shiny, executing, opens Executing",
				}),
			).toBeInTheDocument();
		});

		test("selects the latest role", async () => {
			const user = userEvent.setup();
			for (const [plan, role] of [
				[scopingPlan(), "scoping"],
				[executingPlan(), "executing"],
				[landingPlan(), "landing"],
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

		test("keeps plain styling once selected", () => {
			const plan = executingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			const header = screen.getByRole("button", {
				name: "Shiny, executing, opens Executing",
			});
			expect(header.className).toBe("plan-name");
		});

		test("shows cancel left of an automatic switch", () => {
			const plan = scopingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "scoping" })}
				/>,
			);
			const group = planGroup("Parallel sessions, scoping, opens Scoping");
			const buttons = within(group).getAllByRole("button");
			const cancel = within(group).getByRole("button", {
				name: "Cancel Parallel sessions",
			});
			const mode = within(group).getByRole("button", {
				name: "Switch to manual",
			});
			expect(mode.textContent).toBe("A");
			expect(mode.className).toContain("mode");
			expect(mode.className).not.toContain("manual");
			expect(buttons.indexOf(cancel)).toBeLessThan(buttons.indexOf(mode));
		});

		test("shows a manual switch with the reverse label", async () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59",
				"scoping",
				"Parallel sessions",
				true,
				[testStatus("scoping")],
				null,
				true,
			);
			const props = panelProps([plan], {
				plan: plan.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			const mode = screen.getByRole("button", { name: "Switch to auto" });
			expect(mode.textContent).toBe("M");
			expect(mode.className).toContain("manual");
			await user.click(mode);
			expect(props.onSetMode).toHaveBeenCalledWith(plan.name, false);
		});

		test("flipping to manual calls back with the plan", async () => {
			const plan = scopingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", { name: "Switch to manual" }),
			);
			expect(props.onSetMode).toHaveBeenCalledWith(plan.name, true);
		});

		test("cancelling from the header uses the latest role", async () => {
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(screen.getByRole("button", { name: "Cancel Shiny" }));
			expect(props.onCancel).toHaveBeenCalledWith({
				plan: plan.name,
				role: "executing",
			});
		});

		test("finished plans show no header controls", () => {
			render(
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
			expect(screen.queryByRole("button", { name: /Switch to/ })).toBeNull();
			expect(screen.queryByRole("button", { name: /Cancel Done/ })).toBeNull();
		});
	});

	describe("expansion", () => {
		test("collapsed plans expose only their header", () => {
			render(<PlansPanel {...panelProps([executingPlan()], null)} />);
			expect(screen.getByText("Shiny")).toBeInTheDocument();
			expect(screen.queryByText("Scoping")).toBeNull();
			expect(screen.queryByText("Executing")).toBeNull();
		});

		test("expanded executing plans list both sessions", () => {
			const plan = executingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			expect(screen.getByText("Scoping")).toBeInTheDocument();
			expect(screen.getByText("Executing")).toBeInTheDocument();
		});

		test("expanded landing plans list three sessions without buttons", () => {
			const plan = landingPlan();
			const { container } = render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "landing" })}
				/>,
			);
			expect(screen.getByText("Scoping")).toBeInTheDocument();
			expect(screen.getByText("Executing")).toBeInTheDocument();
			expect(screen.getByText("Landing")).toBeInTheDocument();
			expect(container.querySelectorAll(".sbody .sbtn").length).toBe(0);
		});

		test("shows session rows only on the expanded plan", () => {
			const active = executingPlan();
			const { container } = render(
				<PlansPanel
					{...panelProps([scopingPlan(), active], {
						plan: active.name,
						role: "executing",
					})}
				/>,
			);
			expect(container.querySelectorAll(".sbtn").length).toBeGreaterThan(0);
			expect(
				within(
					planGroup("Parallel sessions, scoping, opens Scoping"),
				).queryByText("Scoping"),
			).toBeNull();
			expect(
				within(
					planGroup("Parallel sessions, scoping, opens Scoping"),
				).getByRole("button", { name: "Cancel Parallel sessions" }),
			).toBeInTheDocument();
		});

		test("keeps collapsed attention while viewing another plan", () => {
			const idle = testEntryWith("idle", "scoping", "Idle", true, [
				testStatus("scoping"),
			]);
			const active = executingPlan();
			const { container } = render(
				<PlansPanel
					{...panelProps([idle, active], {
						plan: active.name,
						role: "executing",
					})}
				/>,
			);
			expect(screen.getByText("Idle")).toBeInTheDocument();
			expect(
				planGroup("Idle, scoping, opens Scoping").querySelector(".mk-idle"),
			).not.toBeNull();
			expect(container.querySelectorAll(".mk-idle").length).toBeGreaterThan(0);
		});
	});

	describe("attention", () => {
		test("shows only plans waiting on the user", () => {
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
			expect(container.querySelector(".mk-idle")).not.toBeNull();
			expect(container.querySelector(".mk-approval")).not.toBeNull();
			expect(container.querySelector(".mk-failed")).not.toBeNull();
			expect(
				container.querySelectorAll(".mk-idle, .mk-approval, .mk-failed").length,
			).toBe(3);
			expect(container.querySelectorAll(".mk-none").length).toBe(2);
		});

		test("names the cause on markers", () => {
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
			const tips = [
				...container.querySelectorAll(".mk-idle, .mk-approval, .mk-failed"),
			].map((node) => node.getAttribute("title"));
			expect(tips).toEqual([
				"needs input",
				"needs approval",
				"failed, needs a response",
			]);
		});

		test("marks the selected quiet plan white and others hollow", () => {
			const quiet = testEntryWith("run", "scoping", "Run", true, [
				testStatus("scoping", { working: true }),
			]);
			const idle = testEntryWith("idle", "scoping", "Idle", true, [
				testStatus("scoping"),
			]);
			const { container } = render(
				<PlansPanel
					{...panelProps([idle, quiet], { plan: "run", role: "scoping" })}
				/>,
			);
			expect(
				planGroup("Run, scoping, opens Scoping").querySelector(".mk-sel"),
			).not.toBeNull();
			expect(container.querySelector(".mk-sel")).not.toBeNull();
		});

		test("sweeps attention names with a sheen", () => {
			render(
				<PlansPanel
					{...panelProps([
						testEntryWith("idle", "scoping", "Idle", true, [
							testStatus("scoping"),
						]),
						testEntryWith("run", "scoping", "Run", true, [
							testStatus("scoping", { working: true }),
						]),
					])}
				/>,
			);
			expect(
				planGroup("Idle, scoping, opens Scoping").querySelector(
					".ptitle.sheen-idle",
				),
			).not.toBeNull();
			expect(
				planGroup("Run, scoping, opens Scoping").querySelector(".ptitle")
					?.className,
			).toBe("ptitle");
		});
	});

	describe("session rows", () => {
		test("rows select without offering step buttons", () => {
			const plan = executingPlan();
			const { container } = render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			expect(screen.getByText("Scoping")).toBeInTheDocument();
			expect(screen.getByText("Executing")).toBeInTheDocument();
			expect(container.querySelectorAll(".sbody .sbtn").length).toBe(0);
			expect(
				screen.queryByRole("button", { name: /Send|Land|Start landing/i }),
			).toBeNull();
		});

		test("finished plans have no buttons", () => {
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
			expect(
				planGroup("Done, completed, opens Executing").querySelectorAll(".sbtn")
					.length,
			).toBe(0);
			expect(container.querySelectorAll(".sbtn").length).toBe(1);
		});
	});

	describe("actions", () => {
		test("header cancel calls back with the latest session", async () => {
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(screen.getByRole("button", { name: "Cancel Shiny" }));
			expect(props.onCancel).toHaveBeenCalledWith({
				plan: plan.name,
				role: "executing",
			});
			expect(props.onSetMode).not.toHaveBeenCalled();
		});

		test("selecting a row marks it selected", async () => {
			const active = executingPlan();
			const props = panelProps([scopingPlan(), active], {
				plan: active.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(screen.getByRole("button", { name: "Shiny Executing" }));
			expect(props.onSelect).toHaveBeenCalledWith({
				plan: active.name,
				role: "executing",
			});
		});

		test("action buttons carry no tooltips", () => {
			const active = executingPlan();
			const { container } = render(
				<PlansPanel
					{...panelProps([scopingPlan(), active], {
						plan: active.name,
						role: "executing",
					})}
				/>,
			);
			for (const button of container.querySelectorAll(".sbtn")) {
				expect(button.getAttribute("title")).toBeNull();
				expect(
					button.textContent === "✕" ||
						button.textContent === "A" ||
						button.textContent === "M" ||
						button.textContent === "+",
				).toBe(true);
			}
		});
	});

	describe("styling", () => {
		test("session labels render muted", () => {
			const label = /\.sbody\s+\.slabel\s*\{[^}]*\}/.exec(appCss())?.[0] ?? "";
			expect(label).toContain("#9d9a92");
		});

		test("phase headers match the TODOS green", () => {
			const head = /\.sect-head\s*\{[^}]*\}/.exec(appCss())?.[0] ?? "";
			expect(head).toContain("#7dffc4");
			expect(head).toContain("11px");
			expect(head).toContain("0.14em");
		});

		test("attention markers stay square", () => {
			const css = appCss();
			expect(css).not.toContain(".adot");
			expect(css).not.toContain(".app *");
			expect(css).not.toContain("border-radius: 50%");
			const reset =
				/\.app[^{]*\{[^}]*border-radius[^}]*\}/.exec(css)?.[0] ?? "";
			expect(reset).toContain(":not(");
			expect(reset).toContain(".mk");
		});

		test("the select button fills the whole row", () => {
			const row = /\.sbody\s+\.srow\s*\{[^}]*\}/.exec(appCss())?.[0] ?? "";
			expect(row).toContain("align-self: stretch");
		});

		test("action buttons stay hover-only except on the selected plan", () => {
			const css = appCss();
			expect(css).not.toContain(".session");
			expect(css).not.toContain(".newplan");
			expect(css).not.toContain(".sbtn.promote");
			const hidden = /\.plans\s+\.sbtn\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(hidden).toContain("opacity: 0");
			const shown =
				/\.plans\s+\.plan-group[^{]*\.sbtn[^{]*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(shown).toContain("opacity: 1");
		});

		test("header controls stay hover-only except on the selected plan", () => {
			const css = appCss();
			expect(css).toContain(".plan-group:hover .hact .sbtn");
			expect(css).toContain(".plan-group.sel .hact .sbtn");
			expect(css).toContain(".sect-head:hover .sbtn");
		});

		test("mode switch uses green auto and amber manual", () => {
			const css = appCss();
			const auto = /\.sbtn\.mode\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(auto).toContain("#7dffc4");
			const manual = /\.sbtn\.mode\.manual\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(manual).toContain("#ffd98a");
		});

		test("the tree uses a dim spine without TODOS green", () => {
			const css = appCss();
			expect(css).toContain("rgba(255, 255, 255, 0.22)");
			const tree = /\.plan-group\.tree\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(tree).toContain("18px");
			const capped = /\.ngutter\.last\s+\.v\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(capped).toContain("50%");
			const single = /\.ngutter\.single\s+\.v\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(single).toContain("50%");
		});
	});

	describe("tree", () => {
		test("draws a gutter per session with a capped spine", () => {
			const plan = executingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			const group = planGroup("Shiny, executing, opens Executing");
			expect(group.querySelectorAll(".ngutter").length).toBe(2);
			expect(group.querySelectorAll(".mgutter").length).toBe(1);
			const gutters = group.querySelectorAll(".ngutter");
			expect(gutters.item(0)?.className).not.toContain("last");
			expect(gutters.item(1)?.className).toContain("last");
		});

		test("caps a single session spine", () => {
			const plan = scopingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "scoping" })}
				/>,
			);
			const group = planGroup("Parallel sessions, scoping, opens Scoping");
			expect(group.querySelectorAll(".ngutter").length).toBe(1);
			expect(group.querySelector(".ngutter.single")).not.toBeNull();
		});

		test("colors the open path when the open session causes it", () => {
			const plan = testEntryWith("idle", "scoping", "Idle", true, [
				testStatus("scoping"),
			]);
			render(
				<PlansPanel
					{...panelProps([plan], { plan: "idle", role: "scoping" })}
				/>,
			);
			const group = planGroup("Idle, scoping, opens Scoping");
			expect(group.querySelector(".mgutter .v.c-idle")).not.toBeNull();
			expect(group.querySelector(".ngutter .v.c-idle")).not.toBeNull();
			expect(group.querySelector(".ngutter .nd.c-idle")).not.toBeNull();
		});

		test("keeps the open path white when another session causes it", () => {
			const plan = testEntryWith("broke", "executing", "Broke", true, [
				testStatus("scoping"),
				testStatus("executing", { failed: true }),
			]);
			render(
				<PlansPanel
					{...panelProps([plan], { plan: "broke", role: "scoping" })}
				/>,
			);
			const group = planGroup("Broke, executing, opens Executing");
			expect(group.querySelector(".mk-failed")).not.toBeNull();
			expect(group.querySelector(".mgutter .v.c-sel")).not.toBeNull();
			expect(group.querySelector(".ngutter .nd.sel")).not.toBeNull();
			expect(group.querySelector(".ngutter .nd.c-failed")).toBeNull();
		});

		test("keeps the open path white for quiet plans with a white marker", () => {
			const plan = testEntryWith("run", "scoping", "Run", true, [
				testStatus("scoping", { working: true }),
			]);
			render(
				<PlansPanel
					{...panelProps([plan], { plan: "run", role: "scoping" })}
				/>,
			);
			const group = planGroup("Run, scoping, opens Scoping");
			expect(group.querySelector(".mk-sel")).not.toBeNull();
			expect(group.querySelector(".mgutter .v.c-sel")).not.toBeNull();
			expect(group.querySelector(".ngutter .nd.sel")).not.toBeNull();
		});
	});
});
