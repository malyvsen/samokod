import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { RepoPicker } from "./components/RepoPicker";

describe("repo picker", () => {
	test("lists recent repos with open actions", () => {
		render(
			<RepoPicker
				title="SAMOKOD"
				subtitle="open a git repository to start one chat"
				recent={[
					{ path: "~/code/samokod", branch: "main" },
					{ path: "~/code/dotfiles", branch: "master" },
				]}
				currentPath={null}
				error={null}
				onOpen={vi.fn()}
				onBrowse={vi.fn()}
				onBack={null}
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
				subtitle="open a git repository to start one chat"
				recent={[{ path: "~/code/samokod", branch: "main" }]}
				currentPath={null}
				error="~/downloads/notes - not a git repository"
				onOpen={vi.fn()}
				onBrowse={vi.fn()}
				onBack={null}
				onDismissError={vi.fn()}
			/>,
		);
		expect(screen.getByRole("alert")).toHaveTextContent("not a git repository");
	});

	test("marks current repo and offers back from chat", () => {
		render(
			<RepoPicker
				title="SAMOKOD"
				subtitle="switch repository - the current chat closes"
				recent={[
					{ path: "~/code/samokod", branch: "main" },
					{ path: "~/code/dotfiles", branch: "master" },
				]}
				currentPath="~/code/samokod"
				error={null}
				onOpen={vi.fn()}
				onBrowse={vi.fn()}
				onBack={vi.fn()}
				onDismissError={vi.fn()}
			/>,
		);
		expect(screen.getByText("current")).toBeInTheDocument();
		expect(screen.getByText("back to chat")).toBeInTheDocument();
		expect(screen.getAllByText("open")).toHaveLength(1);
	});
});
