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
			agentLabel="AGENT"
			onRetry={vi.fn()}
			onAnswer={vi.fn()}
		/>
	);
}

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
