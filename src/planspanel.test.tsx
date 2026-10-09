import { readFileSync } from "node:fs";
import { render, screen, within } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { PlansPanel } from "./components/PlansPanel";
import { testEntryWith, testStatus } from "./fixtures";
import type { PlanEntry } from "./types";

const SECTIONS = [
	"SCOPING",
	"EXECUTING",
	"EVERGREENING",
	"LANDING",
	"COMPLETED",
	"CANCELLED",
];

function panelProps(
	plans: PlanEntry[],
	selected: {
		plan: string;
		role: "scoping" | "executing" | "evergreening" | "landing";
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
		onSetEvergreen: vi.fn(),
	};
}

async function openMenu(
	user: ReturnType<typeof userEvent.setup>,
	planTitle: string,
) {
	await user.click(
		screen.getByRole("button", { name: `Plan options for ${planTitle}` }),
	);
}

function menuOf(planTitle: string) {
	const group = planGroupFor(planTitle);
	const menu = group.querySelector(".pdrop");
	if (menu === null) throw new Error(`menu missing for ${planTitle}`);
	return menu as HTMLElement;
}

function planGroupFor(planTitle: string) {
	const header = screen
		.getAllByRole("button")
		.find((node) => node.textContent?.includes(planTitle)) as
		| HTMLElement
		| undefined;
	if (header === undefined) throw new Error(`plan missing: ${planTitle}`);
	const group = header.closest(".plan-group");
	if (group === null) throw new Error("plan missing");
	return group as HTMLElement;
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
		[
			testStatus("scoping"),
			testStatus("executing"),
			testStatus("evergreening"),
			testStatus("landing"),
		],
	);
}

function evergreeningPlan() {
	return testEntryWith(
		"2026-09-25.10-54-59.green",
		"evergreening",
		"Shiny",
		true,
		[
			testStatus("scoping"),
			testStatus("executing"),
			testStatus("evergreening"),
		],
	);
}

function sectionHeaders() {
	return screen
		.getAllByText(
			/^(SCOPING|EXECUTING|EVERGREENING|LANDING|COMPLETED|CANCELLED)$/,
		)
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

		test("renders all six sections in fixed order", () => {
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
				[evergreeningPlan(), "evergreening", /evergreening, No todos yet$/i],
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

		test("headers expose a single options trigger instead of buttons", () => {
			const plan = scopingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "scoping" })}
				/>,
			);
			const group = planGroup("Parallel sessions, scoping, opens Scoping");
			expect(
				within(group).getByRole("button", {
					name: "Plan options for Parallel sessions",
				}).textContent,
			).toBe("...");
			expect(group.querySelector(".sbtn.execute")).toBeNull();
			expect(group.querySelector(".sbtn.mode")).toBeNull();
			expect(group.querySelector(".sbtn.cancel")).toBeNull();
			expect(
				within(group).queryByRole("menuitem", { name: "Execute" }),
			).toBeNull();
		});

		test("scoping menus hold evergreen, execute, and cancel", async () => {
			const plan = scopingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Parallel sessions");
			const menu = menuOf("Parallel sessions");
			const rows = within(menu)
				.getAllByRole("menuitemcheckbox")
				.map((row) => row.textContent);
			expect(rows).toEqual(["[x]Evergreen"]);
			expect(
				within(menu).getByRole("menuitem", { name: "Execute" }),
			).toBeInTheDocument();
			expect(
				within(menu).getByRole("menuitem", { name: "Cancel" }),
			).toBeInTheDocument();
			expect(
				within(menu).queryByRole("menuitemcheckbox", {
					name: "Auto-advance",
				}),
			).toBeNull();
		});

		test("executing menus hold auto-advance, evergreen, and cancel", async () => {
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Shiny");
			const menu = menuOf("Shiny");
			expect(
				within(menu).getByRole("menuitemcheckbox", { name: "Auto-advance" }),
			).toBeInTheDocument();
			expect(
				within(menu).getByRole("menuitemcheckbox", { name: "Evergreen" }),
			).toBeInTheDocument();
			expect(
				within(menu).getByRole("menuitem", { name: "Cancel" }),
			).toBeInTheDocument();
			expect(
				within(menu).queryByRole("menuitem", { name: "Execute" }),
			).toBeNull();
		});

		test("evergreening and landing menus skip the evergreen row", async () => {
			const user = userEvent.setup();
			for (const plan of [evergreeningPlan(), landingPlan()]) {
				const props = panelProps([plan], {
					plan: plan.name,
					role: plan.phase === "landing" ? "landing" : "evergreening",
				});
				const { unmount } = render(<PlansPanel {...props} />);
				await openMenu(user, "Shiny");
				const menu = menuOf("Shiny");
				expect(
					within(menu).getByRole("menuitemcheckbox", {
						name: "Auto-advance",
					}),
				).toBeInTheDocument();
				expect(
					within(menu).queryByRole("menuitemcheckbox", {
						name: "Evergreen",
					}),
				).toBeNull();
				expect(
					within(menu).getByRole("menuitem", { name: "Cancel" }),
				).toBeInTheDocument();
				unmount();
			}
		});

		test("execute stays disabled without plan.md", async () => {
			const user = userEvent.setup();
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
			await openMenu(user, "Bare");
			expect(
				within(menuOf("Bare")).getByRole("menuitem", { name: "Execute" }),
			).toBeDisabled();
		});

		test("toggles keep the menu open while actions close it", async () => {
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Shiny");
			await user.click(
				screen.getByRole("menuitemcheckbox", { name: "Auto-advance" }),
			);
			expect(props.onSetMode).toHaveBeenCalledWith(plan.name, true);
			expect(
				within(menuOf("Shiny")).getByRole("menuitemcheckbox", {
					name: "Evergreen",
				}),
			).toBeInTheDocument();
			await user.click(
				screen.getByRole("menuitemcheckbox", { name: "Evergreen" }),
			);
			expect(props.onSetEvergreen).toHaveBeenCalledWith(plan.name, false);
			expect(
				within(menuOf("Shiny")).getByRole("menuitem", { name: "Cancel" }),
			).toBeInTheDocument();
			await user.click(screen.getByRole("menuitem", { name: "Cancel" }));
			expect(props.onCancel).toHaveBeenCalledWith({
				plan: plan.name,
				role: "executing",
			});
			expect(
				planGroup("Shiny, executing, No todos yet").querySelector(".pdrop"),
			).toBeNull();
		});

		test("toggling auto-advance back calls back with the plan", async () => {
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
			await openMenu(user, "Shiny");
			const toggle = screen.getByRole("menuitemcheckbox", {
				name: "Auto-advance",
			});
			expect(toggle.textContent).toBe("[ ]Auto-advance");
			await user.click(toggle);
			expect(props.onSetMode).toHaveBeenCalledWith(plan.name, false);
		});

		test("evergreen marks follow intent", async () => {
			const user = userEvent.setup();
			const on = testEntryWith(
				"on",
				"executing",
				"On",
				true,
				[testStatus("scoping"), testStatus("executing")],
				false,
				true,
			);
			const off = testEntryWith(
				"off",
				"executing",
				"Off",
				true,
				[testStatus("scoping"), testStatus("executing")],
				false,
				false,
			);
			const props = panelProps([on, off], null);
			render(<PlansPanel {...props} />);
			await openMenu(user, "On");
			expect(
				within(menuOf("On")).getByRole("menuitemcheckbox", {
					name: "Evergreen",
				}).textContent,
			).toBe("[x]Evergreen");
			await user.click(
				screen.getByRole("button", { name: "Plan options for On" }),
			);
			await openMenu(user, "Off");
			const toggle = within(menuOf("Off")).getByRole("menuitemcheckbox", {
				name: "Evergreen",
			});
			expect(toggle.textContent).toBe("[ ]Evergreen");
			await user.click(toggle);
			expect(props.onSetEvergreen).toHaveBeenCalledWith("off", true);
		});

		test("menus stay per-row", async () => {
			const props = panelProps([scopingPlan(), executingPlan()], null);
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Parallel sessions");
			expect(
				planGroupFor("Parallel sessions").querySelector(".pdrop"),
			).not.toBeNull();
			expect(
				planGroup("Shiny, executing, No todos yet").querySelector(".pdrop"),
			).toBeNull();
			await user.click(
				screen.getByRole("menuitemcheckbox", { name: "Evergreen" }),
			);
			expect(props.onSetEvergreen).toHaveBeenCalledWith(
				scopingPlan().name,
				false,
			);
			await openMenu(user, "Shiny");
			expect(
				planGroupFor("Parallel sessions").querySelector(".pdrop"),
			).toBeNull();
			expect(
				planGroup("Shiny, executing, No todos yet").querySelector(".pdrop"),
			).not.toBeNull();
			await user.click(
				screen.getByRole("menuitemcheckbox", { name: "Auto-advance" }),
			);
			expect(props.onSetMode).toHaveBeenCalledWith(executingPlan().name, true);
			expect(props.onSetEvergreen).toHaveBeenCalledTimes(1);
		});

		test("escape closes the menu", async () => {
			const user = userEvent.setup();
			render(
				<PlansPanel
					{...panelProps([scopingPlan()], {
						plan: scopingPlan().name,
						role: "scoping",
					})}
				/>,
			);
			await openMenu(user, "Parallel sessions");
			expect(
				planGroupFor("Parallel sessions").querySelector(".pdrop"),
			).not.toBeNull();
			await user.keyboard("{Escape}");
			expect(
				planGroupFor("Parallel sessions").querySelector(".pdrop"),
			).toBeNull();
		});

		test("outside clicks close the menu", async () => {
			const user = userEvent.setup();
			render(
				<PlansPanel {...panelProps([scopingPlan(), executingPlan()], null)} />,
			);
			await openMenu(user, "Parallel sessions");
			expect(
				planGroupFor("Parallel sessions").querySelector(".pdrop"),
			).not.toBeNull();
			await user.click(
				screen.getByRole("button", {
					name: "Shiny, executing, No todos yet",
				}),
			);
			expect(
				planGroupFor("Parallel sessions").querySelector(".pdrop"),
			).toBeNull();
		});

		test("executing from the menu uses the scoping session", async () => {
			const plan = scopingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Parallel sessions");
			await user.click(screen.getByRole("menuitem", { name: "Execute" }));
			expect(props.onExecute).toHaveBeenCalledWith({
				plan: plan.name,
				role: "scoping",
			});
			expect(props.onSetMode).not.toHaveBeenCalled();
		});

		test("cancelling from the menu uses the latest role", async () => {
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Shiny");
			await user.click(screen.getByRole("menuitem", { name: "Cancel" }));
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

		test("expanded landing plans list four sessions without buttons", () => {
			const plan = landingPlan();
			const { container } = render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "landing" })}
				/>,
			);
			expect(screen.getByText("Scoping")).toBeInTheDocument();
			expect(screen.getByText("Executing")).toBeInTheDocument();
			expect(screen.getByText("Evergreening")).toBeInTheDocument();
			expect(screen.getByText("Landing")).toBeInTheDocument();
			expect(container.querySelectorAll(".sbody .sbtn").length).toBe(0);
		});

		test("expanded evergreening plans list three sessions", () => {
			const plan = evergreeningPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "evergreening" })}
				/>,
			);
			expect(screen.getByText("Scoping")).toBeInTheDocument();
			expect(screen.getByText("Executing")).toBeInTheDocument();
			expect(screen.getByText("Evergreening")).toBeInTheDocument();
			expect(screen.queryByText("Landing")).toBeNull();
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
				).getByRole("button", {
					name: "Plan options for Parallel sessions",
				}),
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

		test("hover tooltip shows stacked cost and context", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[
					testStatus("scoping"),
					testStatus("executing", {
						spend: { cost: 1.24, ctx_pct: 38.4 },
						todos: [],
					}),
				],
			);
			const { container } = render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			const tips = container.querySelectorAll(".sess-tip .costline");
			expect(tips.length).toBeGreaterThan(0);
			expect(screen.getAllByText("$1.24").length).toBeGreaterThan(0);
			expect(screen.getAllByText("38% context").length).toBeGreaterThan(0);
		});

		test("hover tooltip lists todos with status marks", () => {
			const plan = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[
					testStatus("scoping"),
					testStatus("executing", {
						spend: { cost: 0.42, ctx_pct: 10 },
						todos: [
							{ content: "Add retry", status: "completed" },
							{ content: "Wire view", status: "in_progress" },
							{ content: "Verify green", status: "pending" },
						],
					}),
				],
			);
			const { container } = render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			expect(screen.getAllByText("Add retry").length).toBeGreaterThan(0);
			expect(screen.getAllByText("Wire view").length).toBeGreaterThan(0);
			const tips = [...container.querySelectorAll(".sess-tip")];
			const doneMark = tips
				.map((tip) => tip.querySelector(".trow.done .mark")?.textContent)
				.find((text) => text !== undefined);
			expect(doneMark).toContain("x");
			const activeMark = tips
				.map((tip) => tip.querySelector(".trow.active .mark")?.textContent)
				.find((text) => text !== undefined);
			expect(activeMark).toContain(">");
			for (const tip of tips) {
				expect(tip.textContent).not.toContain("1/3");
			}
		});

		test("hover tooltip shows empty todos without counts", () => {
			const plan = testEntryWith("empty", "scoping", "Empty", false, [
				testStatus("scoping", { spend: { cost: 0, ctx_pct: 0 }, todos: [] }),
			]);
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "scoping" })}
				/>,
			);
			expect(screen.getAllByText("No todos yet").length).toBeGreaterThan(0);
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
		test("menu cancel calls back with the latest session", async () => {
			const plan = executingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await openMenu(user, "Shiny");
			await user.click(screen.getByRole("menuitem", { name: "Cancel" }));
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
				expect(button.textContent).toBe("+");
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

		test("plan menus float a bordered dropdown", () => {
			const css = appCss();
			const top = /\.phead-top\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(top).toContain("position: relative");
			const drop = /\.pdrop\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(drop).toContain("position: absolute");
			expect(drop).toContain("rgba(8, 8, 12, 0.96)");
			const sep = /\.dsep\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(sep).toContain("rgba(255, 255, 255, 0.12)");
			const mark = /\.pdrop\s+\.mark\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(mark).toContain("26px");
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

		test("mode switch uses green auto and blue manual", () => {
			const css = appCss();
			const auto = /\.sbtn\.mode\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(auto).toContain("#7dffc4");
			const manual = /\.sbtn\.mode\.manual\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(manual).toContain("#57c8ff");
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
