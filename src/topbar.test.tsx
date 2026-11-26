import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { TopBar } from "./components/TopBar";

function topBar(options: { onToggleSettings?: () => void } = {}) {
	return <TopBar onToggleSettings={options.onToggleSettings ?? vi.fn()} />;
}

describe("top bar", () => {
	test("shows no status pill", () => {
		const { container } = render(topBar());
		expect(container.querySelector(".status")).toBeNull();
		expect(screen.queryByText("IDLE")).not.toBeInTheDocument();
		expect(screen.queryByText("● WORKING")).not.toBeInTheDocument();
		expect(screen.queryByText("● PAUSED - APPROVAL")).not.toBeInTheDocument();
		expect(screen.queryByText("● FAILED")).not.toBeInTheDocument();
	});

	test("shows no stop control", () => {
		render(topBar());
		expect(
			screen.queryByRole("button", { name: "STOP" }),
		).not.toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Stop this turn" }),
		).not.toBeInTheDocument();
	});

	test("there is no plan menu", () => {
		render(topBar());
		expect(
			screen.queryByRole("button", { name: /plan phase/ }),
		).not.toBeInTheDocument();
	});

	test("brand is a button that toggles settings", () => {
		const onToggleSettings = vi.fn();
		render(topBar({ onToggleSettings }));
		const brand = screen.getByRole("button", { name: "Toggle settings" });
		expect(brand.textContent).toBe("SAMOKOD");
		expect(brand.className).toContain("brand");
		brand.click();
		expect(onToggleSettings).toHaveBeenCalledTimes(1);
	});
});
