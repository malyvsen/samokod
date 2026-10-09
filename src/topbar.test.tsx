import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { TopBar } from "./components/TopBar";
import type { ChatState } from "./sessions/store";
import { emptyChat } from "./sessions/store";

function chatWith(overrides: Partial<ChatState> = {}): ChatState {
	return { ...emptyChat(), ...overrides };
}

function topBar(
	chat: ChatState | null,
	options: { onStop?: () => void; onToggleSettings?: () => void } = {},
) {
	return (
		<TopBar
			repoLabel="~/repo"
			branch="feature"
			chat={chat}
			onStop={options.onStop ?? vi.fn()}
			onToggleSettings={options.onToggleSettings ?? vi.fn()}
		/>
	);
}

describe("top bar", () => {
	test("remounts the status on swap", () => {
		const view = render(topBar(chatWith()));
		const idle = screen.getByText("IDLE");
		view.rerender(topBar(chatWith({ working: true })));
		expect(screen.queryByText("IDLE")).not.toBeInTheDocument();
		expect(screen.getByText("● WORKING")).toBeInTheDocument();
		expect(view.container.querySelectorAll(".status")).toHaveLength(1);
		expect(screen.getByText("● WORKING")).not.toBe(idle);
	});

	test("approval shows paused status without working pulse", () => {
		render(topBar(chatWith({ approval: true })));
		const label = screen.getByText("● PAUSED - APPROVAL");
		expect(label).toHaveClass("paused");
		expect(label).not.toHaveClass("live");
	});

	test("failed shows a red pill", () => {
		render(topBar(chatWith({ failed: true })));
		const label = screen.getByText("● FAILED");
		expect(label).toHaveClass("failed");
		expect(label).not.toHaveClass("live");
	});

	test("idle shows no stop control", () => {
		render(topBar(chatWith()));
		expect(screen.getByText("IDLE")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});

	test("failed shows no stop control", () => {
		render(topBar(chatWith({ failed: true })));
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});

	test("null chat shows idle without stop", () => {
		render(topBar(null));
		expect(screen.getByText("IDLE")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});

	test("replaying keeps idle pill but shows stop", () => {
		render(topBar(chatWith({ start: { kind: "replaying" } })));
		expect(screen.getByText("IDLE")).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "STOP" })).toBeInTheDocument();
	});

	test("preparing keeps idle pill without stop", () => {
		render(topBar(chatWith({ start: { kind: "preparing" } })));
		expect(screen.getByText("IDLE")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
	});

	test.each([{ working: true }, { approval: true }] as const)(
		"stops the turn from the top bar (%s)",
		(overrides) => {
			const onStop = vi.fn();
			render(topBar(chatWith(overrides), { onStop }));
			const stop = screen.getByRole("button", { name: "STOP" });
			expect(stop.parentElement).toHaveClass("stop-wrap");
			stop.click();
			expect(onStop).toHaveBeenCalledTimes(1);
		},
	);

	test("repo chip is static text", () => {
		const { container } = render(topBar(chatWith()));
		const chip = container.querySelector(".repo-static");
		expect(chip?.textContent).toBe("~/repo · feature");
		expect(
			screen.queryByRole("button", { name: /repo/ }),
		).not.toBeInTheDocument();
	});

	test("repo chip carries no bold segment", () => {
		const { container } = render(topBar(chatWith()));
		expect(
			container.querySelector(".repo-static b, .repo-static strong"),
		).toBeNull();
	});

	test("there is no plan menu", () => {
		render(topBar(chatWith()));
		expect(
			screen.queryByRole("button", { name: /plan phase/ }),
		).not.toBeInTheDocument();
	});

	test("brand is a button that toggles settings", () => {
		const onToggleSettings = vi.fn();
		render(topBar(chatWith(), { onToggleSettings }));
		const brand = screen.getByRole("button", { name: "Toggle settings" });
		expect(brand.textContent).toBe("SAMOKOD");
		expect(brand.className).toContain("brand");
		brand.click();
		expect(onToggleSettings).toHaveBeenCalledTimes(1);
	});
});
