import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { RepoPicker } from "./components/RepoPicker";

describe("repo picker", () => {
	test("lists recent repos with open actions", () => {
		render(
			<RepoPicker
				title="SAMOKOD"
				subtitle="open a git repository to start planning"
				recent={[{ path: "~/code/samokod" }, { path: "~/code/dotfiles" }]}
				error={null}
				onOpen={vi.fn()}
				onBrowse={vi.fn()}
				onDismissError={vi.fn()}
			/>,
		);
		expect(screen.getByText("~/code/samokod")).toBeInTheDocument();
		expect(screen.getAllByText("open")).toHaveLength(2);
		expect(screen.getByText("browse…")).toBeInTheDocument();
	});

	test("shows inline error for non-git picks", () => {
		render(
			<RepoPicker
				title="SAMOKOD"
				subtitle="open a git repository to start planning"
				recent={[{ path: "~/code/samokod" }]}
				error="~/downloads/notes - not a git repository"
				onOpen={vi.fn()}
				onBrowse={vi.fn()}
				onDismissError={vi.fn()}
			/>,
		);
		expect(screen.getByRole("alert")).toHaveTextContent("not a git repository");
	});
});
