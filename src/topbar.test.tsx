import { render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { TopBar } from "./components/TopBar";
import { testPlan } from "./fixtures";
import type { AgentStatus, PlanInfo } from "./types";

function topBar(
	status: AgentStatus,
	options: {
		onStop?: () => void;
		onExecute?: () => void;
		onComplete?: () => void;
		onAbandon?: () => void;
		plan?: PlanInfo | null;
	} = {},
) {
	return (
		<TopBar
			repoLabel="repo"
			branch="main"
			status={status}
			plan={options.plan ?? null}
			onOpenPicker={vi.fn()}
			onStop={options.onStop ?? vi.fn()}
			onExecute={options.onExecute ?? vi.fn()}
			onComplete={options.onComplete ?? vi.fn()}
			onAbandon={options.onAbandon ?? vi.fn()}
		/>
	);
}

describe("top bar", () => {
	test("remounts the status on swap", () => {
		const view = render(topBar("idle"));
		const idle = screen.getByText("IDLE");
		view.rerender(topBar("working"));
		expect(screen.queryByText("IDLE")).not.toBeInTheDocument();
		expect(screen.getByText("● WORKING")).toBeInTheDocument();
		expect(view.container.querySelectorAll(".status")).toHaveLength(1);
		expect(screen.getByText("● WORKING")).not.toBe(idle);
	});

	test("approval shows paused status without working pulse", () => {
		render(topBar("approval"));
		const label = screen.getByText("● PAUSED - APPROVAL");
		expect(label).toHaveClass("paused");
		expect(label).not.toHaveClass("live");
	});

	test("idle shows no stop control", () => {
		render(topBar("idle"));
		expect(screen.getByText("IDLE")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});

	test.each(["working", "approval"] as const)(
		"%s stops the turn from the top bar",
		(status) => {
			const onStop = vi.fn();
			render(topBar(status, { onStop }));
			const stop = screen.getByRole("button", { name: "STOP" });
			expect(stop.parentElement).toHaveClass("stop-wrap");
			stop.click();
			expect(onStop).toHaveBeenCalledTimes(1);
		},
	);

	test("without a plan there is no phase menu", () => {
		render(topBar("idle"));
		expect(
			screen.queryByRole("button", { name: /plan phase/ }),
		).not.toBeInTheDocument();
	});

	test("scoping without plan.md disables execute with a visible hint", async () => {
		const user = userEvent.setup();
		render(topBar("idle", { plan: testPlan("scoping", false) }));
		await user.click(
			screen.getByRole("button", { name: "plan phase scoping" }),
		);
		const execute = screen.getByRole("button", {
			name: "Execute Needs plan.md",
		});
		expect(execute).toBeDisabled();
		expect(screen.getByText("Needs plan.md")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Abandon" })).toBeInTheDocument();
	});

	test("scoping with plan.md enables execute", async () => {
		const user = userEvent.setup();
		const onExecute = vi.fn();
		const { rerender } = render(topBar("idle"));
		rerender(topBar("idle", { onExecute, plan: testPlan("scoping", true) }));
		await user.click(
			screen.getByRole("button", { name: "plan phase scoping" }),
		);
		await user.click(screen.getByRole("button", { name: "Execute" }));
		expect(onExecute).toHaveBeenCalledTimes(1);
	});

	test("executing offers mark completed and abandon", async () => {
		const user = userEvent.setup();
		render(topBar("idle", { plan: testPlan("executing", true) }));
		await user.click(
			screen.getByRole("button", { name: "plan phase executing" }),
		);
		expect(
			screen.getByRole("button", { name: "Mark completed" }),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "Abandon" })).toBeInTheDocument();
	});
});
