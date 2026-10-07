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
		onExecute: vi.fn(),
		onCancel: vi.fn(),
		onFinishLanding: vi.fn(),
		onBeginLanding: vi.fn(),
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

function sessionRow(buttonName: string) {
	const row = screen
		.getByRole("button", { name: buttonName })
		.closest(".session");
	if (row === null) throw new Error("row missing");
	return within(row as HTMLElement);
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

		test("expanded landing plans list three sessions with cancel and finish", () => {
			const plan = landingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "landing" })}
				/>,
			);
			expect(screen.getByText("Scoping")).toBeInTheDocument();
			expect(screen.getByText("Executing")).toBeInTheDocument();
			expect(screen.getByText("Landing")).toBeInTheDocument();
			const buttons = sessionRow("Shiny Landing");
			expect(
				buttons.getByRole("button", {
					name: "Cancel landing and delete branch",
				}),
			).toBeInTheDocument();
			expect(
				buttons.getByRole("button", {
					name: `Finish landing ${plan.name}`,
				}),
			).toBeInTheDocument();
		});

		test("shows actions only on the expanded session", () => {
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
				).queryByRole("button", { name: /Cancel|Send|Land/i }),
			).toBeNull();
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
					".ptitle.gleam-idle",
				),
			).not.toBeNull();
			expect(
				planGroup("Run, scoping, opens Scoping").querySelector(".ptitle")
					?.className,
			).toBe("ptitle");
		});
	});

	describe("session rows", () => {
		test("scoping rows offer cancel and execute", () => {
			const plan = scopingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "scoping" })}
				/>,
			);
			const buttons = sessionRow("Parallel sessions Scoping");
			expect(
				buttons.getByRole("button", { name: `Cancel ${plan.name}` }),
			).toBeInTheDocument();
			expect(
				buttons.getByRole("button", {
					name: `Send ${plan.name} to execution`,
				}),
			).toBeInTheDocument();
		});

		test("executing rows offer cancel and done", () => {
			const plan = executingPlan();
			render(
				<PlansPanel
					{...panelProps([plan], { plan: plan.name, role: "executing" })}
				/>,
			);
			const buttons = sessionRow("Shiny Executing");
			expect(
				buttons.getByRole("button", {
					name: `Cancel ${plan.name} and delete branch`,
				}),
			).toBeInTheDocument();
			expect(
				buttons.getByRole("button", {
					name: `Land ${plan.name} onto feature`,
				}),
			).toBeInTheDocument();
		});

		test("diverged executing rows offer landing instead of done", () => {
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
						plan: diverged.name,
						role: "executing",
					})}
				/>,
			);
			expect(
				screen.getByRole("button", {
					name: `Start landing ${diverged.name} onto feature`,
				}),
			).toBeInTheDocument();
			expect(
				screen.queryByRole("button", {
					name: `Land ${diverged.name} onto feature`,
				}),
			).toBeNull();
		});

		test("dirty executing rows disable landing with a tooltip", () => {
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
					{...panelProps([dirty], { plan: dirty.name, role: "executing" })}
				/>,
			);
			const land = screen.getByRole("button", {
				name: `Land ${dirty.name} onto feature`,
			});
			expect(land).toBeDisabled();
			expect(land.getAttribute("title")).toBe(
				"Commit or discard worktree changes first",
			);
		});

		test("inactive rows have no buttons", () => {
			const active = executingPlan();
			render(
				<PlansPanel
					{...panelProps(
						[
							active,
							testEntryWith("done", "completed", "Done", true, [
								testStatus("scoping"),
								testStatus("executing"),
							]),
							testEntryWith("drop", "cancelled", "Drop", false, [
								testStatus("scoping"),
							]),
						],
						{ plan: active.name, role: "executing" },
					)}
				/>,
			);
			expect(
				sessionRow("Shiny Scoping").queryByRole("button", {
					name: /Cancel|Send|Mark/,
				}),
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
	});

	describe("actions", () => {
		test("row actions call back with the session key", async () => {
			const plan = scopingPlan();
			const props = panelProps([plan], {
				plan: plan.name,
				role: "scoping",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", {
					name: `Send ${plan.name} to execution`,
				}),
			);
			expect(props.onExecute).toHaveBeenCalledWith({
				plan: plan.name,
				role: "scoping",
			});
			await user.click(
				screen.getByRole("button", { name: `Cancel ${plan.name}` }),
			);
			expect(props.onCancel).toHaveBeenCalledWith({
				plan: plan.name,
				role: "scoping",
			});
		});

		test("row actions never act on the wrong session", async () => {
			const active = executingPlan();
			const props = panelProps([scopingPlan(), active], {
				plan: active.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", {
					name: `Land ${active.name} onto feature`,
				}),
			);
			expect(props.onFinishLanding).toHaveBeenCalledWith({
				plan: active.name,
				role: "executing",
			});
			expect(props.onCancel).not.toHaveBeenCalled();
		});

		test("landing actions call back with the executing session", async () => {
			const diverged = testEntryWith(
				"2026-09-25.10-54-59.slug",
				"executing",
				"Shiny",
				true,
				[testStatus("scoping"), testStatus("executing")],
				testWorktree({ ffable: false }),
			);
			const props = panelProps([diverged], {
				plan: diverged.name,
				role: "executing",
			});
			const user = userEvent.setup();
			render(<PlansPanel {...props} />);
			await user.click(
				screen.getByRole("button", {
					name: `Start landing ${diverged.name} onto feature`,
				}),
			);
			expect(props.onBeginLanding).toHaveBeenCalledWith({
				plan: diverged.name,
				role: "executing",
			});
			expect(props.onFinishLanding).not.toHaveBeenCalled();
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
						button.textContent === "✓",
				).toBe(true);
			}
		});
	});

	describe("styling", () => {
		test("session labels render muted", () => {
			const label =
				/\.session\s+\.slabel\s*\{[^}]*\}/.exec(appCss())?.[0] ?? "";
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
			const row = /\.session\s+\.srow\s*\{[^}]*\}/.exec(appCss())?.[0] ?? "";
			expect(row).toContain("align-self: stretch");
		});

		test("action buttons stay hover-only", () => {
			const css = appCss();
			const hidden = /\.plans\s+\.sbtn\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(hidden).toContain("opacity: 0");
			const shown =
				/\.plans\s+\.session:hover\s+\.sbtn[^{]*\{[^}]*\}/.exec(css)?.[0] ?? "";
			expect(shown).toContain("opacity: 1");
		});
	});
});
