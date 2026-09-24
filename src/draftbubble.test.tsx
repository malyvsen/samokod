import { fireEvent, render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { describe, expect, test, vi } from "vitest";
import { DraftBubble, extractDraftText } from "./components/DraftBubble";

function bubble(props: { onSend?: (text: string) => void } = {}) {
	const onSend = props.onSend ?? vi.fn();
	render(<DraftBubble onSend={onSend} onEdit={vi.fn()} />);
	return { area: screen.getByRole("textbox"), onSend };
}

function typeLines(area: HTMLElement, text: string) {
	area.focus();
	area.textContent = text;
	const range = document.createRange();
	range.selectNodeContents(area);
	range.collapse(false);
	const selection = window.getSelection();
	selection?.removeAllRanges();
	selection?.addRange(range);
}

describe("draft bubble", () => {
	test("autofocuses on mount", () => {
		const { area } = bubble();
		expect(document.activeElement).toBe(area);
	});

	test("Enter sends and clears", () => {
		const { area, onSend } = bubble();
		typeLines(area, "hello");
		fireEvent.keyDown(area, { key: "Enter" });
		expect(onSend).toHaveBeenCalledWith("hello");
		expect(area.textContent).toBe("");
	});

	test("Shift+Enter does not send", () => {
		const { area, onSend } = bubble();
		typeLines(area, "hello");
		fireEvent.keyDown(area, { key: "Enter", shiftKey: true });
		expect(onSend).not.toHaveBeenCalled();
	});

	test("composing Enter does not send", () => {
		const { area, onSend } = bubble();
		typeLines(area, "hello");
		const event = new KeyboardEvent("keydown", {
			key: "Enter",
			bubbles: true,
			cancelable: true,
		});
		Object.defineProperty(event, "isComposing", { value: true });
		fireEvent(area, event);
		expect(onSend).not.toHaveBeenCalled();
	});

	test("empty draft is ignored", () => {
		const { area, onSend } = bubble();
		typeLines(area, "   \n  ");
		fireEvent.keyDown(area, { key: "Enter" });
		expect(onSend).not.toHaveBeenCalled();
	});

	test("paste inserts plain text with line breaks", () => {
		const { area } = bubble();
		area.focus();
		fireEvent.paste(area, {
			clipboardData: { getData: () => "first\nsecond" },
		});
		expect(area.textContent).toContain("first\nsecond");
		expect(area.querySelectorAll("*")).toHaveLength(0);
	});

	test("realistic typing stays sendable", async () => {
		const user = userEvent.setup();
		const { area, onSend } = bubble();
		await user.click(area);
		await user.keyboard("line one{Shift>}{Enter}{/Shift}line two{Enter}");
		expect(onSend).toHaveBeenCalledWith("line one\nline two");
	});
});

describe("extractDraftText", () => {
	function root(html: string) {
		const node = document.createElement("div");
		node.innerHTML = html;
		return node;
	}

	test("keeps internal newlines, trims the ends", () => {
		expect(extractDraftText(root("a\nb"))).toBe("a\nb");
		expect(extractDraftText(root("\n  a\nb  \n"))).toBe("a\nb");
	});

	test("turns pasted blocks into lines", () => {
		expect(extractDraftText(root("<div>a</div><div>b</div>"))).toBe("a\nb");
		expect(extractDraftText(root("a<br>b"))).toBe("a\nb");
		expect(extractDraftText(root("a<br>"))).toBe("a");
	});

	test("blank edits read as empty", () => {
		expect(extractDraftText(root(""))).toBe("");
		expect(extractDraftText(root("<br>"))).toBe("");
		expect(extractDraftText(root("<div><br></div>"))).toBe("");
	});
});
