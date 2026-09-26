import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { testWorktree } from "../../fixtures";
import type { PlanPhase, SessionRole } from "../../types";
import { actionKind, RowActions } from "./RowActions";

describe("actionKind", () => {
	test("live rows map to their own kind", () => {
		expect(actionKind("scoping", "scoping")).toBe("scoping");
		expect(actionKind("executing", "executing")).toBe("executing");
		expect(actionKind("merging", "merging")).toBe("merging");
	});

	test("finished and history rows map to history", () => {
		const finished: Array<[PlanPhase, SessionRole]> = [
			["completed", "scoping"],
			["completed", "executing"],
			["completed", "merging"],
			["cancelled", "scoping"],
			["cancelled", "executing"],
			["cancelled", "merging"],
			["executing", "scoping"],
			["executing", "merging"],
			["merging", "scoping"],
			["merging", "executing"],
			["scoping", "executing"],
			["scoping", "merging"],
		];
		for (const [phase, role] of finished) {
			expect(actionKind(phase, role)).toBe("history");
		}
	});
});

describe("row actions", () => {
	test("scoping rows offer abandon and execute", () => {
		render(
			<RowActions
				kind="scoping"
				planName="plan"
				hasPlanMd={true}
				running={false}
				onExecute={vi.fn()}
				onAbandon={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Abandon plan" }),
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
				onDone={vi.fn()}
				onBeginMerge={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Cancel plan and delete branch" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Merge plan to main" }),
		).toBeInTheDocument();
	});

	test("diverged executing rows offer rebase instead of done", () => {
		render(
			<RowActions
				kind="executing"
				planName="plan"
				running={false}
				worktree={testWorktree({ ffable: false })}
				onCancel={vi.fn()}
				onDone={vi.fn()}
				onBeginMerge={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", {
				name: "Rebase plan onto latest main",
			}),
		).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Merge plan to main" }),
		).toBeNull();
	});

	test("dirty worktrees disable the merge button with a tooltip", () => {
		render(
			<RowActions
				kind="executing"
				planName="plan"
				running={false}
				worktree={testWorktree({ dirty: true })}
				onCancel={vi.fn()}
				onDone={vi.fn()}
				onBeginMerge={vi.fn()}
			/>,
		);
		const merge = screen.getByRole("button", { name: "Merge plan to main" });
		expect(merge).toBeDisabled();
		expect(merge.getAttribute("title")).toBe(
			"Commit or discard worktree changes first",
		);
	});

	test("merging rows offer cancel and finish", () => {
		render(
			<RowActions
				kind="merging"
				planName="plan"
				running={false}
				dirty={false}
				onCancel={vi.fn()}
				onDone={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Cancel merge and delete branch" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Finish plan merge" }),
		).toBeInTheDocument();
	});

	test("dirty merging rows disable finish with a tooltip", () => {
		render(
			<RowActions
				kind="merging"
				planName="plan"
				running={false}
				dirty={true}
				onCancel={vi.fn()}
				onDone={vi.fn()}
			/>,
		);
		const finish = screen.getByRole("button", { name: "Finish plan merge" });
		expect(finish).toBeDisabled();
		expect(finish.getAttribute("title")).toBe(
			"Commit or discard worktree changes first",
		);
	});

	test("history rows render nothing", () => {
		const { container } = render(<RowActions kind="history" planName="plan" />);
		expect(container).toBeEmptyDOMElement();
	});
});
