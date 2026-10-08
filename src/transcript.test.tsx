import { readFileSync } from "node:fs";
import { render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { Transcript } from "./components/Transcript";
import type { SessionStart } from "./sessions/store";

const multiline = "first line\nsecond line\n- third";
const IDLE: SessionStart = { kind: "idle" };

function transcript(text: string, start: SessionStart | null = IDLE) {
	return (
		<Transcript
			items={[{ kind: "user", id: "u1", text }]}
			start={start}
			repoLabel="~/repo"
			onRetry={vi.fn()}
			onAnswer={vi.fn()}
		/>
	);
}

function agentTranscript(text: string, start: SessionStart | null = IDLE) {
	return (
		<Transcript
			items={[{ kind: "agent", id: "a1", text }]}
			start={start}
			repoLabel="~/repo"
			onRetry={vi.fn()}
			onAnswer={vi.fn()}
		/>
	);
}

function emptyTranscript(start: SessionStart | null) {
	return (
		<Transcript
			items={[]}
			start={start}
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

describe("transcript start copy", () => {
	test("missing entry shows opening session", () => {
		render(emptyTranscript(null));
		expect(screen.getByText(/opening session/)).toBeInTheDocument();
		expect(screen.getByText("getting session ready")).toBeInTheDocument();
	});

	test("preparing shows fresh session preparing", () => {
		render(emptyTranscript({ kind: "preparing" }));
		expect(screen.getByText(/fresh session/)).toBeInTheDocument();
		expect(screen.getByText("preparing session")).toBeInTheDocument();
	});

	test("replaying shows loading history", () => {
		render(emptyTranscript({ kind: "replaying" }));
		expect(screen.getByText(/loading history/)).toBeInTheDocument();
		expect(screen.getByText("replaying past messages")).toBeInTheDocument();
	});

	test("idle shows fresh session no messages", () => {
		render(emptyTranscript({ kind: "idle" }));
		expect(screen.getByText(/fresh session/)).toBeInTheDocument();
		expect(screen.getByText("no messages yet")).toBeInTheDocument();
	});

	test("non-empty transcripts render messages regardless of phase", () => {
		render(transcript("hello", { kind: "replaying" }));
		expect(screen.getByText("hello")).toBeInTheDocument();
		expect(
			screen.queryByText("replaying past messages"),
		).not.toBeInTheDocument();
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
				start={IDLE}
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
				start={IDLE}
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

describe("transcript lead", () => {
	test("lead renders first as a YOU bubble", () => {
		const { container } = render(
			<Transcript
				items={[{ kind: "user", id: "u1", text: "hello" }]}
				lead="TEMPLATE"
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		const bubbles = container.querySelectorAll(".msg.user");
		expect(bubbles).toHaveLength(2);
		expect(bubbles[0]?.textContent).toContain("TEMPLATE");
		expect(bubbles[1]?.textContent).toContain("hello");
	});

	test("absent lead renders nothing extra", () => {
		const { container } = render(transcript("hello"));
		expect(container.querySelectorAll(".msg.user")).toHaveLength(1);
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

describe("transcript thought rows", () => {
	test("frozen bursts read as thinking seconds", () => {
		render(
			<Transcript
				items={[{ kind: "thought", id: "t1", seconds: 8 }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		expect(screen.getByText("thinking: 8s")).toBeInTheDocument();
	});

	test("waiting shows a static row", () => {
		const { container } = render(
			<Transcript
				items={[{ kind: "user", id: "u1", text: "hello" }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				live={{ kind: "waiting" }}
			/>,
		);
		expect(container.querySelector(".arow.waiting")?.textContent).toBe(
			"waiting",
		);
	});

	test("thinking shows the live tail with the full text tooltip", () => {
		const { container } = render(
			<Transcript
				items={[{ kind: "user", id: "u1", text: "hello" }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				live={{
					kind: "thinking",
					tail: "checking the directory",
					burstStart: 0,
					updatedAt: 1000,
				}}
			/>,
		);
		const row = container.querySelector(".arow.thinking");
		expect(row?.textContent).toContain("thinking:");
		expect(row?.textContent).toContain("checking the directory");
		expect(container.querySelector(".tscroll")?.getAttribute("data-full")).toBe(
			"checking the directory",
		);
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
				start={IDLE}
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
				start={IDLE}
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
