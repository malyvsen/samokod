import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { testWorktree } from "../../fixtures";
import type { PlanPhase, SessionRole } from "../../types";
import { actionKind, RowActions } from "./RowActions";

describe("actionKind", () => {
	test("live rows map to their own kind", () => {
		expect(actionKind("scoping", "scoping")).toBe("scoping");
		expect(actionKind("executing", "executing")).toBe("executing");
		expect(actionKind("landing", "landing")).toBe("landing");
	});

	test("finished and inactive rows map to inactive", () => {
		const finished: Array<[PlanPhase, SessionRole]> = [
			["completed", "scoping"],
			["completed", "executing"],
			["completed", "landing"],
			["cancelled", "scoping"],
			["cancelled", "executing"],
			["cancelled", "landing"],
			["executing", "scoping"],
			["executing", "landing"],
			["landing", "scoping"],
			["landing", "executing"],
			["scoping", "executing"],
			["scoping", "landing"],
		];
		for (const [phase, role] of finished) {
			expect(actionKind(phase, role)).toBe("inactive");
		}
	});
});

describe("row actions", () => {
	test("scoping rows offer cancel and execute", () => {
		render(
			<RowActions
				kind="scoping"
				planName="plan"
				hasPlanMd={true}
				running={false}
				onExecute={vi.fn()}
				onCancel={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Cancel plan" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Send plan to execution" }),
		).toBeInTheDocument();
	});

	test("executing rows offer cancel and done", () => {
		render(
			<RowActions
				kind="executing"
				planName="plan"
				running={false}
				worktree={testWorktree()}
				onCancel={vi.fn()}
				onFinishLanding={vi.fn()}
				onBeginLanding={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Cancel plan and delete branch" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Land plan onto feature" }),
		).toBeInTheDocument();
	});

	test("diverged executing rows offer landing instead of done", () => {
		render(
			<RowActions
				kind="executing"
				planName="plan"
				running={false}
				worktree={testWorktree({ ffable: false })}
				onCancel={vi.fn()}
				onFinishLanding={vi.fn()}
				onBeginLanding={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", {
				name: "Start landing plan onto feature",
			}),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Land plan onto feature" }),
		).toBeNull();
	});

	test("dirty worktrees disable the landing button with a tooltip", () => {
		render(
			<RowActions
				kind="executing"
				planName="plan"
				running={false}
				worktree={testWorktree({ dirty: true })}
				onCancel={vi.fn()}
				onFinishLanding={vi.fn()}
				onBeginLanding={vi.fn()}
			/>,
		);
		const land = screen.getByRole("button", { name: "Land plan onto feature" });
		expect(land).toBeDisabled();
		expect(land.getAttribute("title")).toBe(
			"Commit or discard worktree changes first",
		);
	});

	test("landing rows offer cancel and finish", () => {
		render(
			<RowActions
				kind="landing"
				planName="plan"
				running={false}
				dirty={false}
				onCancel={vi.fn()}
				onFinishLanding={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Cancel landing and delete branch" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Finish landing plan" }),
		).toBeInTheDocument();
	});

	test("dirty landing rows disable finish with a tooltip", () => {
		render(
			<RowActions
				kind="landing"
				planName="plan"
				running={false}
				dirty={true}
				onCancel={vi.fn()}
				onFinishLanding={vi.fn()}
			/>,
		);
		const finish = screen.getByRole("button", { name: "Finish landing plan" });
		expect(finish).toBeDisabled();
		expect(finish.getAttribute("title")).toBe(
			"Commit or discard worktree changes first",
		);
	});

	test("inactive rows render nothing", () => {
		const { container } = render(
			<RowActions kind="inactive" planName="plan" />,
		);
		expect(container).toBeEmptyDOMElement();
	});
});
