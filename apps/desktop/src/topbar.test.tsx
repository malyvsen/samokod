import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { TopBar } from "./components/TopBar";
import type { AgentStatus } from "./types";

function topBar(status: AgentStatus) {
	return (
		<TopBar
			repoLabel="repo"
			branch="main"
			status={status}
			onOpenPicker={vi.fn()}
			onNewChat={vi.fn()}
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
});
