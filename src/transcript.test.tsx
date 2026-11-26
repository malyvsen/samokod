import { readFileSync } from "node:fs";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, test, vi } from "vitest";
import { Transcript } from "./components/Transcript";
import { groupRuns, summarizeRun } from "./components/workGroups";
import type { SessionStart } from "./sessions/store";
import type { RunItem } from "./types";

const multiline = "first line\nsecond line\n- third";
const IDLE: SessionStart = { kind: "idle" };
const SESSION = { plan: "a", role: "scoping" } as const;

function tool(id: string, label: string, at?: number): RunItem {
	return {
		kind: "tool",
		id,
		line: { id, text: `${label}: body`, status: "completed", label },
		...(at === undefined ? {} : { at }),
	};
}

function thought(id: string, at?: number, ms = 1000): RunItem {
	return {
		kind: "thought",
		id,
		ms,
		text: "hmm",
		...(at === undefined ? {} : { at }),
	};
}

function transcript(text: string, start: SessionStart | null = IDLE) {
	return (
		<Transcript
			items={[{ kind: "user", id: "u1", text }]}
			start={start}
			repoLabel="~/repo"
			onRetry={vi.fn()}
			onAnswer={vi.fn()}
			session={SESSION}
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
			session={SESSION}
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
			session={SESSION}
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
				session={SESSION}
			/>,
		);
		fireEvent.click(screen.getByTitle("Expand"));
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
				session={SESSION}
			/>,
		);
		fireEvent.click(screen.getByTitle("Expand"));
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
				session={SESSION}
			/>,
		);
		const bubbles = container.querySelectorAll(".msg.user");
		expect(bubbles).toHaveLength(1);
		expect(bubbles[0]?.textContent).toContain("TEMPLATE");
	});

	test("lead renders first alongside a different user text", () => {
		const { container } = render(
			<Transcript
				items={[{ kind: "user", id: "u1", text: "hello" }]}
				lead="TEMPLATE"
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
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
	test("frozen thoughts read as thinking seconds", () => {
		render(
			<Transcript
				items={[{ kind: "thought", id: "t1", ms: 8000, text: "hmm" }]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(screen.getByText("thinking: 8s")).toBeInTheDocument();
	});

	test("adjacent thoughts stay as separate rows", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 1500, text: "first" },
					{ kind: "thought", id: "t2", ms: 1500, text: "second" },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(2);
		expect(rows[0]?.textContent).toBe("thinking: 2s");
		expect(rows[1]?.textContent).toBe("thinking: 2s");
	});

	test("separated thoughts stay separate", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 8000, text: "first" },
					{ kind: "agent", id: "a1", text: "hello" },
					{ kind: "thought", id: "t2", ms: 8000, text: "second" },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(2);
		expect(rows[0]?.textContent).toBe("thinking: 8s");
		expect(rows[1]?.textContent).toBe("thinking: 8s");
	});

	test("trailing box of a working session ticks wall-clock", () => {
		vi.useFakeTimers();
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(5000);
		try {
			const { container } = render(
				<Transcript
					items={[
						{
							kind: "tool",
							id: "t1",
							line: {
								id: "l1",
								text: "read: a",
								status: "completed",
								label: "read",
							},
							at: 1000,
						},
						{
							kind: "tool",
							id: "t2",
							line: {
								id: "l2",
								text: "read: b",
								status: "completed",
								label: "read",
							},
							at: 2000,
						},
					]}
					start={IDLE}
					repoLabel="~/repo"
					onRetry={vi.fn()}
					onAnswer={vi.fn()}
					turn={{ since: 1000, live: null }}
					session={SESSION}
				/>,
			);
			const summary = () =>
				container.querySelector(".work .summary .txt")?.textContent ?? "";
			expect(summary()).toContain("Working for 4s");
			nowSpy.mockReturnValue(6000);
			act(() => {
				vi.advanceTimersByTime(1000);
			});
			expect(summary()).toContain("Working for 5s");
		} finally {
			nowSpy.mockRestore();
			vi.useRealTimers();
		}
	});

	test("frozen box shows static wall-clock", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "l1",
							text: "read: a",
							status: "completed",
							label: "read",
						},
						at: 1000,
					},
					{
						kind: "tool",
						id: "t2",
						line: {
							id: "l2",
							text: "read: b",
							status: "completed",
							label: "read",
						},
						at: 2000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(
			container.querySelector(".work .summary .txt")?.textContent,
		).toContain("Worked for 1s");
	});

	test("thoughts split only on intervening content", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "thought", id: "t1", ms: 2000, text: "first", at: 1000 },
					{
						kind: "tool",
						id: "tool1",
						line: {
							id: "l1",
							text: "read: a",
							status: "completed",
							label: "read",
						},
						at: 2000,
					},
					{ kind: "thought", id: "t2", ms: 1000, text: "second", at: 3000 },
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(container.querySelectorAll(".work")).toHaveLength(1);
		const rows = container.querySelectorAll(".arow.thought");
		expect(rows).toHaveLength(2);
		expect(rows[0]?.textContent).toContain("thinking: 2s");
		expect(rows[1]?.textContent).toContain("thinking: 1s");
	});
});

describe("transcript grouping", () => {
	test("splits only on user and agent messages", () => {
		const groups = groupRuns([
			{ kind: "user", id: "u1", text: "hi" },
			thought("t1"),
			tool("l1", "read"),
			{ kind: "agent", id: "a1", text: "done" },
			thought("t2"),
		]);
		expect(groups).toHaveLength(4);
		expect(groups[0]).toMatchObject({ kind: "message" });
		expect(groups[1]).toMatchObject({ kind: "run" });
		expect(groups[2]).toMatchObject({ kind: "message" });
		expect(groups[3]).toMatchObject({ kind: "run" });
		if (groups[1]?.kind === "run") {
			expect(groups[1].items).toHaveLength(2);
		}
	});

	test("single-item runs still group", () => {
		const groups = groupRuns([
			{ kind: "user", id: "u1", text: "hi" },
			thought("t1"),
			{ kind: "user", id: "u2", text: "again" },
		]);
		expect(groups).toHaveLength(3);
		expect(groups[1]).toMatchObject({ kind: "run" });
		if (groups[1]?.kind === "run") {
			expect(groups[1].items).toHaveLength(1);
		}
	});

	test("counts sort count-desc with alphabetical ties", () => {
		const { counts } = summarizeRun([
			tool("l1", "read", 1000),
			tool("l2", "read", 2000),
			tool("l3", "edit", 3000),
			thought("t1", 4000),
		]);
		expect(counts).toBe("2x read, 1x edit, 1x think");
	});

	test("tool labels come from the label field", () => {
		const { counts } = summarizeRun([tool("l1", "bash", 1000)]);
		expect(counts).toBe("1x bash");
	});

	test("think items are counted", () => {
		const { counts } = summarizeRun([thought("t1", 1000), thought("t2", 2000)]);
		expect(counts).toBe("2x think");
	});

	test("duration hides when no arrival time is known", () => {
		expect(
			summarizeRun([tool("l1", "read", 1000), tool("l2", "read", 3000)])
				.durationMs,
		).toBe(2000);
		expect(
			summarizeRun([tool("l1", "read"), tool("l2", "read")]).durationMs,
		).toBeNull();
		expect(summarizeRun([thought("t1")]).durationMs).toBeNull();
	});

	test("duration counts thought streaming time", () => {
		expect(summarizeRun([thought("t1", 16000, 16000)]).durationMs).toBe(16000);
		expect(
			summarizeRun([thought("t1", 5000, 3000), tool("l1", "read", 5000)])
				.durationMs,
		).toBe(3000);
	});

	test("duration prefers wall-clock span over thought time", () => {
		expect(
			summarizeRun([tool("l1", "read", 1000), thought("t1", 5000, 1000)])
				.durationMs,
		).toBe(4000);
	});

	test("live duration measures from inferred thought start", () => {
		expect(
			summarizeRun([thought("t1", 5000, 3000)], { now: 6000 }).durationMs,
		).toBe(4000);
	});

	test("live duration starts from box items, not ack", () => {
		expect(
			summarizeRun([tool("l1", "read", 64000), tool("l2", "read", 65000)], {
				now: 65000,
				since: 1000,
			}).durationMs,
		).toBe(1000);
	});

	test("collapsed box expands to today's rows", () => {
		const { container } = render(
			<Transcript
				items={[
					{ kind: "user", id: "u1", text: "hi" },
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "l1",
							text: "read: src/types.ts",
							status: "completed",
							label: "read",
						},
						at: 1000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		const box = container.querySelector(".work");
		expect(box).not.toBeNull();
		expect(box?.querySelector(".summary")?.textContent).toContain("1x read");
		const toggle = box?.querySelector(".summary");
		expect(toggle?.getAttribute("aria-expanded")).toBe("false");
		if (toggle instanceof HTMLElement) {
			fireEvent.click(toggle);
		}
		expect(toggle?.getAttribute("aria-expanded")).toBe("true");
		expect(
			container.querySelector(".work .detail .tool")?.textContent,
		).toContain("read: src/types.ts");
	});

	test("todos counted by status", () => {
		const todos = (
			status: "pending" | "in_progress" | "completed",
		): RunItem => ({
			kind: "todos",
			id: `id-${status}`,
			changes: [{ content: "do it", status }],
			at: 1000,
		});
		expect(
			summarizeRun([todos("pending"), todos("in_progress"), todos("completed")])
				.counts,
		).toBe("1x active todo, 1x done todo, 1x new todo");
	});

	test("approvals counted once each", () => {
		const approval = (id: string): RunItem => ({
			kind: "approval",
			id,
			permission: {
				tool_call_id: id,
				title: "run it",
				tool: "bash",
				options: [],
				rule_hint: "hint",
			},
			resolved: false,
			at: 1000,
		});
		expect(summarizeRun([approval("a1"), approval("a2")]).counts).toBe(
			"2x approval",
		);
	});

	test("failed counts errors and failed tools", () => {
		const failedTool: RunItem = {
			kind: "tool",
			id: "t1",
			line: { id: "l1", text: "bash: boom", status: "failed", label: "bash" },
			at: 1000,
		};
		const error: RunItem = {
			kind: "error",
			id: "e1",
			raw: "boom",
			hint: "retry",
			retryable: true,
			at: 2000,
		};
		const { counts, hasFailed } = summarizeRun([failedTool, error]);
		expect(counts).toContain("2x failed");
		expect(counts).toContain("1x bash");
		expect(hasFailed).toBe(true);
	});

	test("failed runs get a red border", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "l1",
							text: "bash: boom",
							status: "failed",
							label: "bash",
						},
						at: 1000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(container.querySelector(".work.failed")).not.toBeNull();
		expect(container.querySelector(".work .summary")?.textContent).toContain(
			"1x failed",
		);
	});

	test("todos render as plain rows without the card", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "todos",
						id: "td1",
						changes: [
							{ content: "write it", status: "pending" },
							{ content: "write it", status: "completed" },
						],
						at: 1000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		fireEvent.click(screen.getByTitle("Expand"));
		expect(container.querySelector(".todo")).toBeNull();
		expect(container.querySelector(".work .detail")?.textContent).toContain(
			"todo new: write it",
		);
		expect(container.querySelector(".work .detail")?.textContent).toContain(
			"todo done: write it",
		);
	});

	test("approvals render as plain blocks without hint or rule", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "approval",
						id: "a1",
						permission: {
							tool_call_id: "tc1",
							title: "bash: run tests",
							tool: "bash",
							options: [
								{ id: "allow1", kind: "allow" },
								{ id: "reject1", kind: "reject" },
							],
							rule_hint: "hint",
						},
						resolved: false,
						at: 1000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(screen.getByTitle("Collapse")).toBeInTheDocument();
		expect(container.querySelector(".approval")).toBeNull();
		const detail = container.querySelector(".work .detail");
		expect(detail?.textContent).toContain("bash: run tests");
		expect(detail?.textContent).not.toContain("Reject skips");
		expect(detail?.querySelector(".hint")).toBeNull();
		expect(
			screen.getByRole("button", { name: "allow once" }),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "reject" })).toBeInTheDocument();
	});
});

describe("transcript row expansion", () => {
	test("think rows toggle full text with titles", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "thought",
						id: "t1",
						ms: 2000,
						text: "first line\nsecond line",
						at: 1000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		fireEvent.click(screen.getByTitle("Expand"));
		const thinkButton = screen.getByTitle("Show full thought");
		expect(thinkButton.textContent).toContain("thinking: 2s");
		const full = container.querySelector(".thinkfull");
		expect(full?.getAttribute("hidden")).not.toBeNull();
		fireEvent.click(thinkButton);
		expect(screen.getByTitle("Hide full thought")).toBeInTheDocument();
		expect(
			container.querySelector(".thinkfull")?.getAttribute("hidden"),
		).toBeNull();
		expect(container.querySelector(".thinkfull")?.textContent).toContain(
			"first line",
		);
		fireEvent.click(screen.getByTitle("Hide full thought"));
		expect(screen.getByTitle("Show full thought")).toBeInTheDocument();
	});

	test("tool rows toggle unwrap with titles", () => {
		const longCommand =
			"bash: git log --oneline --decorate --all -30 -- src/components/Transcript.tsx";
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "l1",
							text: longCommand,
							status: "completed",
							label: "bash",
						},
						at: 1000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		fireEvent.click(screen.getByTitle("Expand"));
		const toolButton = screen.getByTitle("Show full command");
		expect(toolButton.textContent).toContain(longCommand);
		expect(toolButton.classList.contains("open")).toBe(false);
		fireEvent.click(toolButton);
		expect(toolButton.getAttribute("title")).toBe("Collapse");
		expect(container.querySelector(".tool.open")).not.toBeNull();
		fireEvent.click(toolButton);
		expect(toolButton.getAttribute("title")).toBe("Show full command");
	});

	test("box expand keeps working alongside row toggles", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "thought",
						id: "t1",
						ms: 2000,
						text: "full thought",
						at: 1000,
					},
					{
						kind: "tool",
						id: "t2",
						line: {
							id: "l2",
							text: "read: a",
							status: "completed",
							label: "read",
						},
						at: 2000,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		const boxToggle = screen.getByTitle("Expand");
		fireEvent.click(boxToggle);
		expect(screen.getByTitle("Show full thought")).toBeInTheDocument();
		fireEvent.click(screen.getByTitle("Show full thought"));
		expect(screen.getByTitle("Hide full thought")).toBeInTheDocument();
		fireEvent.click(screen.getByTitle("Collapse"));
		expect(
			container.querySelector(".work .summary")?.getAttribute("aria-expanded"),
		).toBe("false");
	});

	test("live thought stays open across streaming chunks", () => {
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(5000);
		try {
			const props = {
				start: IDLE,
				repoLabel: "~/repo",
				onRetry: vi.fn(),
				onAnswer: vi.fn(),
				session: SESSION,
			} as const;
			const { container, rerender } = render(
				<Transcript
					{...props}
					items={[{ kind: "user", id: "u1", text: "hi" }]}
					turn={{
						since: 1000,
						live: { startedAt: 4500, updatedAt: 4500, text: "hmm" },
					}}
				/>,
			);
			fireEvent.click(screen.getByTitle("Expand"));
			fireEvent.click(screen.getByTitle("Show full thought"));
			expect(screen.getByTitle("Hide full thought")).toBeInTheDocument();
			nowSpy.mockReturnValue(6000);
			rerender(
				<Transcript
					{...props}
					items={[{ kind: "user", id: "u1", text: "hi" }]}
					turn={{
						since: 1000,
						live: { startedAt: 4500, updatedAt: 6000, text: "hmm plus more" },
					}}
				/>,
			);
			expect(
				container
					.querySelector(".work .summary")
					?.getAttribute("aria-expanded"),
			).toBe("true");
			expect(screen.getByTitle("Hide full thought")).toBeInTheDocument();
			expect(
				container.querySelector(".thinkfull")?.getAttribute("hidden"),
			).toBeNull();
		} finally {
			nowSpy.mockRestore();
		}
	});

	test("box and live thought stay open across live freeze", () => {
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(5000);
		try {
			const props = {
				start: IDLE,
				repoLabel: "~/repo",
				onRetry: vi.fn(),
				onAnswer: vi.fn(),
				session: SESSION,
			} as const;
			const { container, rerender } = render(
				<Transcript
					{...props}
					items={[
						{
							kind: "tool",
							id: "t1",
							line: {
								id: "l1",
								text: "read: a",
								status: "completed",
								label: "read",
							},
							at: 4000,
						},
					]}
					turn={{
						since: 1000,
						live: {
							startedAt: 4500,
							updatedAt: 4500,
							text: "streaming thought",
						},
					}}
				/>,
			);
			fireEvent.click(screen.getByTitle("Expand"));
			fireEvent.click(screen.getByTitle("Show full thought"));
			expect(screen.getByTitle("Hide full thought")).toBeInTheDocument();
			nowSpy.mockReturnValue(6000);
			rerender(
				<Transcript
					{...props}
					items={[
						{
							kind: "tool",
							id: "t1",
							line: {
								id: "l1",
								text: "read: a",
								status: "completed",
								label: "read",
							},
							at: 4000,
						},
						{
							kind: "thought",
							id: "frozen-1",
							ms: 1500,
							text: "streaming thought",
							at: 6000,
						},
						{
							kind: "tool",
							id: "t2",
							line: {
								id: "l2",
								text: "read: b",
								status: "completed",
								label: "read",
							},
							at: 6000,
						},
					]}
					turn={{ since: 1000, live: null }}
				/>,
			);
			expect(
				container
					.querySelector(".work .summary")
					?.getAttribute("aria-expanded"),
			).toBe("true");
			expect(screen.getByTitle("Hide full thought")).toBeInTheDocument();
			expect(
				container.querySelector(".thinkfull")?.getAttribute("hidden"),
			).toBeNull();
		} finally {
			nowSpy.mockRestore();
		}
	});

	test("frozen box mctl toggles detail and summary still toggles", () => {
		const { container } = render(
			<Transcript
				items={[thought("t1", 1000), tool("t2", "read", 2000)]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		const summary = container.querySelector(
			".work .summary",
		) as HTMLElement | null;
		const detail = () => container.querySelector(".work .detail");
		expect(summary?.getAttribute("aria-expanded")).toBe("false");
		expect(detail()?.getAttribute("hidden")).not.toBeNull();
		const mctl = screen.getByRole("button", { name: "Expand message" });
		expect(mctl.textContent).toBe("+");
		expect(mctl.classList.contains("mctl")).toBe(true);
		fireEvent.click(mctl);
		expect(summary?.getAttribute("aria-expanded")).toBe("true");
		expect(detail()?.getAttribute("hidden")).toBeNull();
		const collapse = screen.getByRole("button", { name: "Collapse message" });
		expect(collapse.textContent).toBe("-");
		fireEvent.click(collapse);
		expect(summary?.getAttribute("aria-expanded")).toBe("false");
		expect(detail()?.getAttribute("hidden")).not.toBeNull();
		fireEvent.click(screen.getByTitle("Expand"));
		expect(summary?.getAttribute("aria-expanded")).toBe("true");
		expect(detail()?.getAttribute("hidden")).toBeNull();
	});

	test("live box shows the same mctl and toggles", () => {
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(5000);
		try {
			const { container } = render(
				<Transcript
					items={[{ kind: "user", id: "u1", text: "hi" }]}
					start={IDLE}
					repoLabel="~/repo"
					onRetry={vi.fn()}
					onAnswer={vi.fn()}
					session={SESSION}
					turn={{
						since: 1000,
						live: { startedAt: 4500, updatedAt: 4500, text: "hmm" },
					}}
				/>,
			);
			const summary = container.querySelector(
				".work .summary",
			) as HTMLElement | null;
			expect(summary?.getAttribute("aria-expanded")).toBe("false");
			const mctl = screen.getByRole("button", { name: "Expand message" });
			expect(mctl.textContent).toBe("+");
			fireEvent.click(mctl);
			expect(summary?.getAttribute("aria-expanded")).toBe("true");
			expect(
				container.querySelector(".work .detail")?.getAttribute("hidden"),
			).toBeNull();
			expect(
				screen.getByRole("button", { name: "Collapse message" }).textContent,
			).toBe("-");
		} finally {
			nowSpy.mockRestore();
		}
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
						line: {
							id: "t1",
							text: "edit: edit file.md",
							status: "completed",
							label: "edit",
						},
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
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
							label: "write",
						},
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
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
		const workRule = /\.work\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(workRule).toContain("width: 100%");
	});

	test("transcript centering stays overflow-safe", () => {
		const css = readFileSync("src/App.css", "utf8");
		const transcriptRule = /\.transcript\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(transcriptRule).not.toMatch(/align-items:\s*center/);
		const columnRule = /\.tcol\s*\{[^}]*\}/.exec(css)?.[0] ?? "";
		expect(columnRule).toMatch(/margin:\s*0\s+auto/);
	});
});

describe("transcript approval sessions", () => {
	function approvalItem(id: string) {
		return {
			kind: "approval" as const,
			id,
			permission: {
				tool_call_id: `tc-${id}`,
				title: `bash: run ${id}`,
				tool: "bash",
				options: [
					{ id: `allow-${id}`, kind: "allow" as const },
					{ id: `reject-${id}`, kind: "reject" as const },
				],
				rule_hint: "hint",
			},
			resolved: false as const,
			at: 1000,
		};
	}

	test("answering passes owning session, not selected", () => {
		const onAnswer = vi.fn();
		const first = { plan: "aaa", role: "scoping" } as const;
		const second = { plan: "bbb", role: "scoping" } as const;
		const { unmount } = render(
			<Transcript
				items={[approvalItem("a1")]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={onAnswer}
				session={first}
			/>,
		);
		expect(screen.getByTitle("Collapse")).toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: "allow once" }));
		expect(onAnswer).toHaveBeenCalledWith(first, "tc-a1", "allow-a1");
		unmount();
		onAnswer.mockClear();
		render(
			<Transcript
				items={[approvalItem("b1")]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={onAnswer}
				session={second}
			/>,
		);
		expect(screen.getByTitle("Collapse")).toBeInTheDocument();
		fireEvent.click(screen.getByRole("button", { name: "allow once" }));
		expect(onAnswer).toHaveBeenCalledWith(second, "tc-b1", "allow-b1");
	});

	test("two sessions render and non-selected card answers owning session", () => {
		const onAnswer = vi.fn();
		const selected = { plan: "aaa", role: "scoping" } as const;
		const background = { plan: "bbb", role: "scoping" } as const;
		render(
			<>
				<div data-testid="selected">
					<Transcript
						items={[approvalItem("a1")]}
						start={IDLE}
						repoLabel="~/repo"
						onRetry={vi.fn()}
						onAnswer={onAnswer}
						session={selected}
					/>
				</div>
				<div data-testid="background">
					<Transcript
						items={[approvalItem("b1")]}
						start={IDLE}
						repoLabel="~/repo"
						onRetry={vi.fn()}
						onAnswer={onAnswer}
						session={background}
					/>
				</div>
			</>,
		);
		const collapses = screen.getAllByTitle("Collapse");
		expect(collapses).toHaveLength(2);
		const allows = screen.getAllByRole("button", { name: "allow once" });
		expect(allows).toHaveLength(2);
		const backgroundAllow = allows[1];
		if (backgroundAllow === undefined) throw new Error("missing allow");
		fireEvent.click(backgroundAllow);
		expect(onAnswer).toHaveBeenCalledTimes(1);
		expect(onAnswer).toHaveBeenCalledWith(background, "tc-b1", "allow-b1");
	});

	test("unresolved approval run renders expanded with both controls", () => {
		const { container } = render(
			<Transcript
				items={[approvalItem("a1")]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
				turn={{ since: 1000, live: null }}
				onStop={vi.fn()}
			/>,
		);
		const detail = container.querySelector(".work .detail");
		expect(detail?.getAttribute("hidden")).toBeNull();
		expect(
			screen.getByRole("button", { name: "allow once" }),
		).toBeInTheDocument();
		expect(screen.getByRole("button", { name: "reject" })).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Review the permission request" }),
		).toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Stop this turn" }),
		).toBeInTheDocument();
	});

	test("resolved approval collapses back to stop-only", () => {
		const resolved = { ...approvalItem("a1"), resolved: true as const };
		const { container } = render(
			<Transcript
				items={[resolved]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
				turn={{ since: 1000, live: null }}
				onStop={vi.fn()}
			/>,
		);
		expect(screen.getByTitle("Expand")).toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Review the permission request" }),
		).not.toBeInTheDocument();
		expect(
			screen.getByRole("button", { name: "Stop this turn" }),
		).toBeInTheDocument();
		const detail = container.querySelector(".work .detail");
		expect(detail?.getAttribute("hidden")).not.toBeNull();
	});
});

describe("transcript live ack", () => {
	function userOnly() {
		return [{ kind: "user" as const, id: "u1", text: "hi" }];
	}

	function agentOnly() {
		return [{ kind: "agent" as const, id: "a1", text: "hello" }];
	}

	test("sent but unacked renders no box", () => {
		const { container } = render(
			<Transcript
				items={userOnly()}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(container.querySelector(".work")).toBeNull();
	});

	test("acked user-only renders a Working box", () => {
		const { container } = render(
			<Transcript
				items={userOnly()}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
				turn={{ since: 1000, live: null }}
				onStop={vi.fn()}
			/>,
		);
		const box = container.querySelector(".work");
		expect(box).not.toBeNull();
		expect(box?.querySelector(".summary .txt")?.textContent).toContain(
			"Working",
		);
	});

	test("empty working placeholder is locked non-expandable", () => {
		const { container } = render(
			<Transcript
				items={userOnly()}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
				turn={{ since: 1000, live: null }}
				onStop={vi.fn()}
			/>,
		);
		const summary = container.querySelector(
			".work .summary",
		) as HTMLElement | null;
		expect(summary?.getAttribute("aria-disabled")).toBe("true");
		fireEvent.click(summary as HTMLElement);
		expect(
			container.querySelector(".work .detail")?.getAttribute("hidden"),
		).not.toBeNull();
		expect(
			container.querySelector(".work .summary")?.getAttribute("aria-expanded"),
		).toBe("false");
		const expanders = screen.queryAllByRole("button", {
			name: "Expand message",
		});
		expect(expanders).toHaveLength(1);
		for (const button of expanders) {
			expect(button).toBeDisabled();
		}
		expect(
			screen.queryByRole("button", { name: "Collapse message" }),
		).not.toBeInTheDocument();
	});

	test("acked pending live joins the trailing run as think", () => {
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(5000);
		try {
			const { container } = render(
				<Transcript
					items={[
						{ kind: "user", id: "u1", text: "hi" },
						{
							kind: "tool",
							id: "t1",
							line: {
								id: "l1",
								text: "read: a",
								status: "completed",
								label: "read",
							},
							at: 4000,
						},
					]}
					start={IDLE}
					repoLabel="~/repo"
					onRetry={vi.fn()}
					onAnswer={vi.fn()}
					session={SESSION}
					turn={{
						since: 1000,
						live: { startedAt: 4500, updatedAt: 4500, text: "hmm" },
					}}
				/>,
			);
			const txt =
				container.querySelector(".work .summary .txt")?.textContent ?? "";
			expect(txt).toContain("Working");
			expect(txt).toContain("1x think");
			expect(txt).toContain("1x read");
		} finally {
			nowSpy.mockRestore();
		}
	});

	test("acked agent-only without live renders no box", () => {
		const { container } = render(
			<Transcript
				items={agentOnly()}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
				turn={{ since: 1000, live: null }}
			/>,
		);
		expect(container.querySelector(".work")).toBeNull();
		expect(screen.getByText("hello")).toBeInTheDocument();
	});

	test("live thought after agent text renders its own trailing box", () => {
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(5000);
		try {
			const { container } = render(
				<Transcript
					items={agentOnly()}
					start={IDLE}
					repoLabel="~/repo"
					onRetry={vi.fn()}
					onAnswer={vi.fn()}
					session={SESSION}
					turn={{
						since: 1000,
						live: { startedAt: 4500, updatedAt: 4500, text: "hmm" },
					}}
				/>,
			);
			const boxes = container.querySelectorAll(".work");
			expect(boxes).toHaveLength(1);
			const txt =
				container.querySelector(".work .summary .txt")?.textContent ?? "";
			expect(txt).toContain("Working");
			expect(txt).toContain("1x think");
		} finally {
			nowSpy.mockRestore();
		}
	});
});

describe("transcript work duration", () => {
	test("placeholder ticks from ack time", () => {
		vi.useFakeTimers();
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(4000);
		try {
			const { container } = render(
				<Transcript
					items={[{ kind: "user", id: "u1", text: "hi" }]}
					start={IDLE}
					repoLabel="~/repo"
					onRetry={vi.fn()}
					onAnswer={vi.fn()}
					session={SESSION}
					turn={{ since: 1000, live: null }}
					onStop={vi.fn()}
				/>,
			);
			const summary = () =>
				container.querySelector(".work .summary .txt")?.textContent ?? "";
			expect(summary()).toContain("Working for 3s");
			expect(summary()).not.toContain("·");
			nowSpy.mockReturnValue(5000);
			act(() => {
				vi.advanceTimersByTime(1000);
			});
			expect(summary()).toContain("Working for 4s");
		} finally {
			nowSpy.mockRestore();
			vi.useRealTimers();
		}
	});

	test("unstamped frozen box omits the duration", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "l1",
							text: "read: a",
							status: "completed",
							label: "read",
						},
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		const txt =
			container.querySelector(".work .summary .txt")?.textContent ?? "";
		expect(txt).toContain("Worked");
		expect(txt).toContain("1x read");
		expect(txt).not.toContain("for");
	});

	test("flooring renders whole seconds", () => {
		const { container } = render(
			<Transcript
				items={[
					{
						kind: "tool",
						id: "t1",
						line: {
							id: "l1",
							text: "read: a",
							status: "completed",
							label: "read",
						},
						at: 1000,
					},
					{
						kind: "tool",
						id: "t2",
						line: {
							id: "l2",
							text: "read: b",
							status: "completed",
							label: "read",
						},
						at: 2500,
					},
				]}
				start={IDLE}
				repoLabel="~/repo"
				onRetry={vi.fn()}
				onAnswer={vi.fn()}
				session={SESSION}
			/>,
		);
		expect(
			container.querySelector(".work .summary .txt")?.textContent,
		).toContain("Worked for 1s");
	});

	test("trailing live box measures its own items, not the turn", () => {
		const nowSpy = vi.spyOn(Date, "now").mockReturnValue(65000);
		try {
			const { container } = render(
				<Transcript
					items={[
						tool("t1", "read", 1000),
						tool("t2", "read", 24000),
						{ kind: "agent", id: "a1", text: "mid" },
						tool("t3", "read", 30000),
						tool("t4", "read", 59000),
						{ kind: "agent", id: "a2", text: "mid again" },
						tool("t5", "read", 64000),
						tool("t6", "read", 65000),
					]}
					start={IDLE}
					repoLabel="~/repo"
					onRetry={vi.fn()}
					onAnswer={vi.fn()}
					session={SESSION}
					turn={{ since: 1000, live: null }}
					onStop={vi.fn()}
				/>,
			);
			const boxes = container.querySelectorAll(".work .summary .txt");
			expect(boxes).toHaveLength(3);
			expect(boxes[0]?.textContent).toContain("Worked for 23s");
			expect(boxes[1]?.textContent).toContain("Worked for 29s");
			expect(boxes[2]?.textContent).toContain("Working for 1s");
			expect(boxes[2]?.textContent).not.toContain("Working for 64s");
		} finally {
			nowSpy.mockRestore();
		}
	});
});
