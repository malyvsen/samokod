import { readFileSync } from "node:fs";
import { render, screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { PlansPanel } from "./components/PlansPanel";
import { testEntryWith, testStatus } from "./fixtures";
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
		onExecute: vi.fn(),
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
	return testEntryWith("2026-09-25.10-54-59.slug", "executing", "Shiny", true, [
		testStatus("scoping"),
		testStatus("executing"),
	]);
}

function landingPlan() {
	return testEntryWith(
		"2026-09-25.10-54-59.landing",
		"landing",
		"Shiny",
		true,
		[testStatus("scoping"), testStatus("executing"), testStatus("landing")],
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
					name: "Shiny, executing, No todos yet",
				}),
			).toBeInTheDocument();
		});

		test("selects the latest role", async () => {
			const user = userEvent.setup();
			for (const [plan, role, name] of [
				[scopingPlan(), "scoping", /opens scoping$/i],
				[executingPlan(), "executing", /executing, No todos yet$/i],
				[landingPlan(), "landing", /opens landing$/i],
			] as const) {
				const props = panelProps([plan], null);
				const { unmount } = render(<PlansPanel {...props} />);
				await user.click(screen.getByRole("button", { name }));
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
				name: "Shiny, executing, No todos yet",
			});
			expect(header.className).toBe("plan-name");
		});

		test("scoping headers offer cancel and execute instead of a switch", () => {
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
			const execute = within(group).getByRole("button", {
				name: "Send Parallel sessions to execution",
			});
			expect(execute.textContent).toBe(">");
			expect(execute.className).toContain("execute");
			expect(buttons.indexOf(cancel)).toBeLessThan(buttons.indexOf(execute));
			expect(
				within(group).queryByRole("button", { name: /Switch to/ }),
			).toBeNull();
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
				screen.getByRole("button", {
					name: "Send Bare to execution",
				}),
			).toBeDisabled();
		});

		test("executing headers offer cancel and the mode switch", () => {
			const plan = executingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			const group = planGroup("Shiny, executing, No todos yet");
			const buttons = within(group).getAllByRole("button");
			const cancel = within(group).getByRole("button", {
				name: "Cancel Shiny",
			});
			const mode = within(group).getByRole("button", {
				name: "Switch to manual",
			});
			expect(mode.textContent).toBe("A");
			expect(mode.className).toContain("mode");
			expect(mode.className).not.toContain("manual");
			expect(buttons.indexOf(cancel)).toBeLessThan(buttons.indexOf(mode));
			expect(
				within(group).queryByRole("button", { name: /to execution/ }),
			).toBeNull();
		});

		test("shows a manual switch with the reverse label", async () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[testStatus("scoping"), testStatus("executing")],
				true,
			);
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
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
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", { name: "Switch to manual" }),
			);
			expect(props.onSetMode).toHaveBeenCalledWith(plan.name, true);
		});

		test("executing from the header uses the scoping session", async () => {
			const plan = scopingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", {
					name: "Send Parallel sessions to execution",
				}),
			);
			expect(props.onExecute).toHaveBeenCalledWith({
				plan: plan.name,
				role: "scoping",
			});
			expect(props.onSetMode).not.toHaveBeenCalled();
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

		test("partial headers show an ETA and a green bar", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[
					testStatus("scoping"),
					testStatus("executing", {
						working: true,
						progress: { done: 2, total: 5, eta_secs: 480 },
					}),
				],
			);
			render(<PlansPanel {...panelProps([plan], null)} />);
			const header = screen.getByRole("button", {
				name: "Shiny, executing, 2 of 5 todos done, 8m left",
			});
			expect(header).toHaveAttribute(
				"data-full",
				"Shiny - 2 of 5 todos done, 8m left",
			);
			const group = header.closest(".plan-group") as HTMLElement;
			expect(within(group).getByText("8m").className).toContain("pmeta");
			expect(within(group).getByText("8m").getAttribute("title")).toBe(
				"2 of 5 todos done, 8m left",
			);
			const bar = group.querySelector(".hbar-in") as HTMLElement;
			expect(bar).not.toBeNull();
			expect(bar.className).not.toContain("paused");
			expect(bar.querySelector("i")?.getAttribute("style")).toContain("40%");
		});

		test("empty headers show no ETA and an empty track", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[testStatus("scoping"), testStatus("executing", { working: true })],
			);
			render(<PlansPanel {...panelProps([plan], null)} />);
			const header = screen.getByRole("button", {
				name: "Shiny, executing, No todos yet",
			});
			expect(header).toHaveAttribute("data-full", "Shiny - No todos yet");
			const group = header.closest(".plan-group") as HTMLElement;
			expect(group.querySelector(".pmeta")).toBeNull();
			expect(
				group.querySelector(".hbar-in i")?.getAttribute("style"),
			).toContain("0%");
		});

		test("estimating headers show no ETA and an empty track", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[
					testStatus("scoping"),
					testStatus("executing", {
						working: true,
						progress: { done: 0, total: 5, eta_secs: null },
					}),
				],
			);
			render(<PlansPanel {...panelProps([plan], null)} />);
			const header = screen.getByRole("button", {
				name: "Shiny, executing, 0 of 5 todos done, estimating time",
			});
			expect(header).toHaveAttribute(
				"data-full",
				"Shiny - 0 of 5 todos done, estimating time",
			);
			const group = header.closest(".plan-group") as HTMLElement;
			expect(group.querySelector(".pmeta")).toBeNull();
			expect(
				group.querySelector(".hbar-in i")?.getAttribute("style"),
			).toContain("0%");
		});

		test("done headers show a full green bar with no ETA", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[
					testStatus("scoping"),
					testStatus("executing", {
						progress: { done: 5, total: 5, eta_secs: null },
					}),
				],
			);
			render(<PlansPanel {...panelProps([plan], null)} />);
			const header = screen.getByRole("button", {
				name: "Shiny, executing, 5 of 5 todos done",
			});
			expect(header).toHaveAttribute("data-full", "Shiny - 5 of 5 todos done");
			const group = header.closest(".plan-group") as HTMLElement;
			expect(group.querySelector(".pmeta")).toBeNull();
			const bar = group.querySelector(".hbar-in") as HTMLElement;
			expect(bar.className).not.toContain("paused");
			expect(bar.querySelector("i")?.getAttribute("style")).toContain("100%");
		});

		test("waiting headers dim the bar but keep the ETA", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[
					testStatus("scoping"),
					testStatus("executing", {
						approval: true,
						progress: { done: 2, total: 5, eta_secs: 480 },
					}),
				],
			);
			render(<PlansPanel {...panelProps([plan], null)} />);
			const header = screen.getByRole("button", {
				name: "Shiny, executing, 2 of 5 todos done, 8m left",
			});
			const group = header.closest(".plan-group") as HTMLElement;
			expect(within(group).getByText("8m")).toBeInTheDocument();
			expect(group.querySelector(".hbar-in")?.className).toContain("paused");
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
				screen.queryByRole("button", { name: /Land|Start landing/i }),
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
						button.textContent === ">" ||
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

		test("header controls stay hover-only without reserving space", () => {
			const css = appCss();
			const hact = /\.hact\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(hact).toContain("display: none");
			expect(css).toContain(".plan-group:hover .hact");
			expect(css).toContain(".plan-group:focus-within .hact");
			expect(css).not.toContain(".plan-group.sel .hact");
			expect(css).toContain(".plan-group:hover .pmeta");
			expect(css).toContain(".plan-group:focus-within .pmeta");
		});

		test("executing headers stack an ETA and a 3px bar", () => {
			const css = appCss();
			const stack = /\.phead\.stack\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(stack).toContain("column");
			const top = /\.phead-top\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(top).toContain("display: flex");
			const meta = /\.pmeta\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(meta).toContain("#6f6c66");
			const bar = /\.hbar-in\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(bar).toContain("3px");
			expect(bar).toContain("rgba(255, 255, 255, 0.14)");
			const fill = /\.hbar-in\s+i\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(fill).toContain("#7dffc4");
			const paused = /\.hbar-in\.paused\s+i\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(paused).toContain("#9d9a92");
		});

		test("mode switch uses green auto and amber manual", () => {
			const css = appCss();
			const auto = /\.sbtn\.mode\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(auto).toContain("#7dffc4");
			const manual = /\.sbtn\.mode\.manual\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(manual).toContain("#ffd98a");
		});

		test("execute uses the same green as new plan", () => {
			const css = appCss();
			const execute = /\.sbtn\.execute\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(execute).toContain("#7dffc4");
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
			const group = planGroup("Shiny, executing, No todos yet");
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
			const group = planGroup("Broke, executing, No todos yet");
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
