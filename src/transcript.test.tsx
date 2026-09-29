import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { Transcript } from "./components/Transcript";

const multiline = "first line\nsecond line\n- third";

function transcript(text: string) {
	return (
		<Transcript
			items={[{ kind: "user", id: "u1", text }]}
			repoLabel="~/repo"
			onRetry={vi.fn()}
			onAnswer={vi.fn()}
		/>
	);
}

function agentTranscript(text: string) {
	return (
		<Transcript
			items={[{ kind: "agent", id: "a1", text }]}
			repoLabel="~/repo"
			onRetry={vi.fn()}
			onAnswer={vi.fn()}
		/>
	);
}

describe("transcript agent header", () => {
	test("agent messages read AI", () => {
		render(agentTranscript("hello"));
		expect(screen.getByText("AI")).toBeInTheDocument();
	});
});

describe("transcript retry", () => {
	test("retryable errors offer retry when writable", () => {
		render(
			<Transcript
				items={[
					{
						kind: "error",
						id: "e1",
						raw: "boom",
						hint: "retry the turn",
						retryable: true,
					},
				]}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		expect(screen.getByRole("button", { name: "retry" })).toBeInTheDocument();
	});

	test("read-only transcripts hide retry", () => {
		render(
			<Transcript
				items={[
					{
						kind: "error",
						id: "e1",
						raw: "boom",
						hint: "retry the turn",
						retryable: true,
					},
				]}
				repoLabel="~/repo"
				onRetry={null}
				onAnswer={vi.fn()}
			/>,
		);
		expect(
			screen.queryByRole("button", { name: "retry" }),
		).not.toBeInTheDocument();
	});
});

describe("transcript user messages", () => {
	test("renders user text with newlines intact", () => {
		render(transcript(multiline));
		const bubble = screen.getByText("YOU").parentElement as HTMLElement;
		expect(bubble.textContent).toContain(multiline);
	});

	test("user bubble rule sets pre-wrap", () => {
		const css = readFileSync("src/App.css", "utf8");
		const rule = /\.msg\.user\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(rule).toContain("white-space: pre-wrap");
	});
});

describe("transcript agent tables", () => {
	const table = "| File | Status |\n| --- | --- |\n| `a.ts` | ok |";

	test("pipe table renders table, header, and body cells in a scroll wrapper", () => {
		const { container } = render(agentTranscript(table));
		const wrap = container.querySelector(".msg.agent .md-table-wrap");
		expect(wrap).not.toBeNull();
		expect(wrap?.querySelector("table")).not.toBeNull();
		expect(
			screen.getByRole("columnheader", { name: "File" }),
		).toBeInTheDocument();
		expect(screen.getByRole("cell", { name: "ok" })).toBeInTheDocument();
		expect(wrap?.querySelector("td code")).not.toBeNull();
	});

	test("delimiter alignment lands on header cells", () => {
		render(
			agentTranscript(
				"| Left | Center | Right |\n| :--- | :----: | ----: |\n| a | b | c |",
			),
		);
		const headers = screen.getAllByRole("columnheader");
		expect(headers[0]).toHaveStyle({ textAlign: "left" });
		expect(headers[1]).toHaveStyle({ textAlign: "center" });
		expect(headers[2]).toHaveStyle({ textAlign: "right" });
	});

	test("pipe text without a delimiter row stays a paragraph", () => {
		const { container } = render(
			agentTranscript("just a | pipe, no table here"),
		);
		expect(container.querySelector(".msg.agent table")).toBeNull();
		expect(screen.getByText(/just a \| pipe/)).toBeInTheDocument();
	});

	test("table cells wrap and wide tables scroll inside the bubble", () => {
		const css = readFileSync("src/App.css", "utf8");
		const wrapRule =
			/\.msg\.agent \.md-table-wrap\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(wrapRule).toContain("overflow-x: auto");
		const cellRule =
			/\.msg\.agent th,\s*\.msg\.agent td\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(cellRule).toContain("white-space: normal");
		expect(cellRule).toContain("overflow-wrap: anywhere");
		expect(cellRule).not.toContain("nowrap");
		expect(cellRule).not.toContain("text-align");
	});
});

describe("transcript tool lines", () => {
	test("renders tool line text as-is", () => {
		render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: { id: "t1", text: "edit: edit file.md", status: "completed" },
					},
				]}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		expect(screen.getByText("edit: edit file.md")).toBeInTheDocument();
		expect(screen.queryByText("edit file.md")).not.toBeInTheDocument();
	});

	test("tool row sets data-status", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "t1",
							text: "write: /var/tool-demo.txt",
							status: "failed",
						},
					},
				]}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		const row = container.querySelector(".tool");
		expect(row?.getAttribute("data-status")).toBe("failed");
		expect(row?.getAttribute("data-full")).toBe("write: /var/tool-demo.txt");
	});

	test("failed tool rule paints red", () => {
		const css = readFileSync("src/App.css", "utf8");
		const rule =
			/\.tool\[data-status="failed"\][^{]*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(rule).toContain("#ff6b6b");
	});
});
