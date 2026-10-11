import { readFileSync } from "node:fs";
import { fireEvent, render, screen } from "@testing-library/react";
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
	test("renders user text as Markdown with newlines intact", () => {
		const { container } = render(transcript(multiline));
		const bubble = screen.getByText("YOU").closest(".msg.user") as HTMLElement;
		expect(bubble.textContent).toContain("first line");
		expect(bubble.textContent).toContain("second line");
		expect(bubble.textContent).toContain("third");
		expect(container.querySelector(".msg.user .body ul li")).not.toBeNull();
	});

	test("message bubbles share base typography and padding", () => {
		const css = readFileSync("src/App.css", "utf8");
		const baseRule = /\.msg\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(baseRule).toContain("font-size: 13px");
		expect(baseRule).toContain("padding: 10px 12px");
		const userRule = /\.msg\.user\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(userRule).not.toContain("white-space: pre-wrap");
		expect(userRule).not.toContain("font-size");
		expect(userRule).not.toContain("padding");
		const agentRule = /\.msg\.agent\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(agentRule).not.toContain("font-size");
		expect(agentRule).not.toContain("padding");
	});

	test("user and agent bubbles share body selectors", () => {
		const md = "# t\n\npara\n\n1. a\n2. b\n\n```\ncode\n```";
		for (const [element, selector] of [
			[transcript, ".msg.user"],
			[agentTranscript, ".msg.agent"],
		] as const) {
			const { container, unmount } = render(element(md));
			expect(container.querySelector(`${selector} .body p`)).not.toBeNull();
			expect(container.querySelector(`${selector} .body ol li`)).not.toBeNull();
			expect(
				container.querySelector(`${selector} .body pre code`),
			).not.toBeNull();
			unmount();
		}
	});

	test("user table renders wrapped table with header cells", () => {
		const table = "| File | Status |\n| --- | --- |\n| `a.ts` | ok |";
		const { container } = render(transcript(table));
		const wrap = container.querySelector(".msg.user .md-table-wrap");
		expect(wrap).not.toBeNull();
		expect(wrap?.querySelector("table")).not.toBeNull();
		expect(
			screen.getByRole("columnheader", { name: "File" }),
		).toBeInTheDocument();
		expect(screen.getByRole("cell", { name: "ok" })).toBeInTheDocument();
	});

	test("user code fence renders pre code", () => {
		const { container } = render(transcript("```\ncode here\n```"));
		const pre = container.querySelector(".msg.user .body pre code");
		expect(pre).not.toBeNull();
		expect(pre?.textContent).toContain("code here");
	});

	test("user pipe text without a delimiter stays a paragraph", () => {
		const { container } = render(transcript("just a | pipe, no table here"));
		expect(container.querySelector(".msg.user table")).toBeNull();
		expect(screen.getByText(/just a \| pipe/)).toBeInTheDocument();
	});
});

describe("transcript lead", () => {
	test("lead hides when a user item already carries exactly that text", () => {
		const { container } = render(
			<Transcript
				items={[{ kind: "user", id: "u1", text: "TEMPLATE" }]}
				lead="TEMPLATE"
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		const bubbles = container.querySelectorAll(".msg.user");
		expect(bubbles).toHaveLength(1);
		expect(bubbles[0]?.textContent).toContain("TEMPLATE");
	});

	test("lead still shows alongside a different user text", () => {
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
			/\.msg \.body \.md-table-wrap\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(wrapRule).toContain("overflow-x: auto");
		const cellRule =
			/\.msg \.body th,\s*\.msg \.body td\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
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
				items={[{ kind: "thought", id: "t1", ms: 8000 }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		expect(screen.getByText("thinking: 8s")).toBeInTheDocument();
	});

	test("adjacent thoughts render one row with summed seconds", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 1500 },
					{ kind: "thought", id: "t2", ms: 1500 },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(1);
		expect(rows[0]?.textContent).toBe("thinking: 3s");
	});

	test("separated thoughts stay separate", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 8000 },
					{ kind: "agent", id: "a1", text: "hello" },
					{ kind: "thought", id: "t2", ms: 8000 },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(2);
		expect(rows[0]?.textContent).toBe("thinking: 8s");
		expect(rows[1]?.textContent).toBe("thinking: 8s");
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

	test("live thinking merges with the trailing run into one total row", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 2000 },
					{ kind: "thought", id: "t2", ms: 1000 },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				live={{ kind: "thinking", burstStart: 0, updatedAt: 1000 }}
			/>,
		);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(1);
		expect(rows[0]?.textContent).toBe("thinking: 4s");
		expect(container.querySelector(".arow.thinking")).toBeNull();
		expect(container.querySelector(".tscroll")).toBeNull();
		expect(container.textContent).not.toContain("checking");
	});

	test("live thinking without preceding thoughts renders the live total", () => {
		const { container } = render(
			<Transcript
				items={[{ kind: "user", id: "u1", text: "hello" }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				live={{ kind: "thinking", burstStart: 0, updatedAt: 2500 }}
			/>,
		);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(1);
		expect(rows[0]?.textContent).toBe("thinking: 3s");
		expect(container.querySelector(".arow.thinking")).toBeNull();
	});

	test("new chunks advance the total while freeze leaves it unchanged", () => {
		const { container, rerender, unmount } = render(
			<Transcript
				items={[{ kind: "thought", id: "t1", ms: 2000 }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				live={{ kind: "thinking", burstStart: 1000, updatedAt: 2000 }}
			/>,
		);
		expect(container.querySelector(".arow.thought")?.textContent).toBe(
			"thinking: 3s",
		);
		rerender(
			<Transcript
				items={[{ kind: "thought", id: "t1", ms: 2000 }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				live={{ kind: "thinking", burstStart: 1000, updatedAt: 3500 }}
			/>,
		);
		expect(container.querySelector(".arow.thought")?.textContent).toBe(
			"thinking: 5s",
		);
		unmount();
		const { container: frozen } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 2000 },
					{ kind: "thought", id: "t2", ms: 2500 },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
			/>,
		);
		expect(frozen.querySelector(".arow.thought")?.textContent).toBe(
			"thinking: 5s",
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

describe("transcript collapse", () => {
	const longUser = `${"long line\n".repeat(11)}final`;
	const longAgent = `${"agent line\n".repeat(11)}final`;

	test("long user starts collapsed and expands on click", () => {
		const { container } = render(transcript(longUser));
		const bubble = container.querySelector(".msg.user");
		expect(bubble?.classList.contains("collapsed")).toBe(true);
		const toggle = screen.getByRole("button", { name: "Expand message" });
		expect(toggle.textContent).toBe("+");
		fireEvent.click(toggle);
		expect(bubble?.classList.contains("collapsed")).toBe(false);
		expect(
			screen.getByRole("button", { name: "Collapse message" }).textContent,
		).toBe("-");
	});

	test("long agent starts expanded and collapses on click", () => {
		const { container } = render(agentTranscript(longAgent));
		const bubble = container.querySelector(".msg.agent");
		expect(bubble?.classList.contains("collapsed")).toBe(false);
		const toggle = screen.getByRole("button", { name: "Collapse message" });
		expect(toggle.textContent).toBe("-");
		fireEvent.click(toggle);
		expect(bubble?.classList.contains("collapsed")).toBe(true);
		expect(
			screen.getByRole("button", { name: "Expand message" }).textContent,
		).toBe("+");
	});

	test("short messages show no collapse button", () => {
		const { container } = render(transcript("hello"));
		expect(container.querySelector(".msg.user")).not.toBeNull();
		expect(
			container.querySelector(".msg.user")?.classList.contains("collapsed"),
		).toBe(false);
		expect(
			screen.queryByRole("button", { name: "Expand message" }),
		).not.toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Collapse message" }),
		).not.toBeInTheDocument();
	});

	test("collapsed CSS caps the body with a fade", () => {
		const css = readFileSync("src/App.css", "utf8");
		const baseRule = /\.msg \.body\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(baseRule).not.toContain("overflow");
		const collapsedRule =
			/\.msg\.collapsed \.body\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(collapsedRule).toContain("max-height");
		expect(collapsedRule).toContain("overflow: hidden");
		expect(css).toContain(".msg.collapsed .body::after");
		expect(css).toContain(".msg .who .right");
	});
});

describe("transcript lists", () => {
	test("message lists leave room for markers", () => {
		const css = readFileSync("src/App.css", "utf8");
		const listRule =
			/\.msg \.body ol,\s*\.msg \.body ul\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(listRule).toContain("padding-left: 28px");
	});

	test("ten-item ordered list renders all items", () => {
		const { container } = render(
			agentTranscript(
				"1. one\n2. two\n3. three\n4. four\n5. five\n6. six\n7. seven\n8. eight\n9. nine\n10. ten",
			),
		);
		expect(container.querySelectorAll(".msg.agent .body ol li")).toHaveLength(
			10,
		);
	});
});

describe("transcript width", () => {
	test("bubbles stay inside the transcript column", () => {
		const css = readFileSync("src/App.css", "utf8");
		const msgRule = /\.msg\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(msgRule).toMatch(/max-width:\s*min\(880px,\s*100%\)/);
		const approvalRule = /\.approval\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(approvalRule).toMatch(/max-width:\s*min\(880px,\s*100%\)/);
	});

	test("transcript centering stays overflow-safe", () => {
		const css = readFileSync("src/App.css", "utf8");
		const transcriptRule = /\.transcript\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(transcriptRule).not.toMatch(/align-items:\s*center/);
		const columnRule = /\.tcol\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(columnRule).toMatch(/margin:\s*0\s+auto/);
	});
});
