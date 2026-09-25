import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { TopBar } from "./components/TopBar";
import type { AgentStatus } from "./types";

function topBar(status: AgentStatus, options: { onStop?: () => void } = {}) {
	return (
		<TopBar
			repoLabel="~/repo"
			branch="main"
			status={status}
			onStop={options.onStop ?? vi.fn()}
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

	test("failed shows a red pill", () => {
		render(topBar("failed"));
		const label = screen.getByText("● FAILED");
		expect(label).toHaveClass("failed");
		expect(label).not.toHaveClass("live");
	});

	test("idle shows no stop control", () => {
		render(topBar("idle"));
		expect(screen.getByText("IDLE")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});

	test("failed shows no stop control", () => {
		render(topBar("failed"));
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

	test("repo chip is static text", () => {
		render(topBar("idle"));
		expect(screen.getByText("~/repo")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: /repo/ }),
		).not.toBeInTheDocument();
	});

	test("there is no plan menu", () => {
		render(topBar("idle"));
		expect(
			screen.queryByRole("button", { name: /plan phase/ }),
		).not.toBeInTheDocument();
	});
});
