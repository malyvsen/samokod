import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import type { PlanPhase, SessionRole } from "../../types";
import { actionKind, RowActions } from "./RowActions";

describe("actionKind", () => {
	test("live rows map to their own kind", () => {
		expect(actionKind("scoping", "scoping")).toBe("scoping");
		expect(actionKind("executing", "executing")).toBe("executing");
	});

	test("finished and history rows map to history", () => {
		const finished: Array<[PlanPhase, SessionRole]> = [
			["completed", "scoping"],
			["completed", "executing"],
			["cancelled", "scoping"],
			["cancelled", "executing"],
			["executing", "scoping"],
			["scoping", "executing"],
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
				onCancel={vi.fn()}
				onDone={vi.fn()}
			/>,
		);
		expect(
			screen.getByRole("button", { name: "Cancel plan" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Mark plan done" }),
		).toBeInTheDocument();
	});

	test("history rows render nothing", () => {
		const { container } = render(<RowActions kind="history" planName="plan" />);
		expect(container).toBeEmptyDOMElement();
	});
});
