import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { saveAttachment } from "./api";
import { MAX_BYTES } from "./components/attachments";
import { DraftBubble } from "./components/DraftBubble";
import { testKey } from "./fixtures";

vi.mock("./api", () => ({ saveAttachment: vi.fn() }));

beforeEach(() => {
	vi.mocked(saveAttachment).mockReset();
});

function bubble(
	props: {
		onSend?: (text: string) => void;
		initialText?: string;
		onInput?: (text: string) => void;
		onAttachmentError?: (message: string) => void;
	} = {},
) {
	const onSend = props.onSend ?? vi.fn();
	render(
		<DraftBubble
			initialText={props.initialText}
			session={testKey()}
			onSend={onSend}
			onEdit={vi.fn()}
			onInput={props.onInput}
			onAttachmentError={props.onAttachmentError ?? vi.fn()}
		/>,
	);
	return { area: screen.getByRole("textbox"), onSend };
}

function pngFile(name = "shot.png"): File {
	const bytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
	const file = new File([bytes], name, { type: "image/png" });
	// jsdom File lacks arrayBuffer; the app runs in a real webview where it
	// exists, so polyfill per file in tests only.
	(file as unknown as { arrayBuffer: () => Promise<ArrayBuffer> }).arrayBuffer =
		async () => bytes.buffer as ArrayBuffer;
	return file;
}

function setCaretAtEnd(area: HTMLElement): void {
	area.focus();
	const range = document.createRange();
	range.selectNodeContents(area);
	range.collapse(false);
	const selection = window.getSelection();
	selection?.removeAllRanges();
	selection?.addRange(range);
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
			clipboardData: { getData: () => "first\nsecond", files: [], items: [] },
		});
		expect(area.textContent).toContain("first\nsecond");
		expect(area.querySelectorAll("*")).toHaveLength(0);
	});

	test("paste with image saves and inserts the bare path", async () => {
		const onInput = vi.fn();
		const { area } = bubble({ onInput });
		setCaretAtEnd(area);
		vi.mocked(saveAttachment).mockResolvedValue(
			"/plan/attachments/image-1-0.png",
		);
		fireEvent.paste(area, {
			clipboardData: {
				getData: () => "",
				files: [pngFile()],
				items: [],
			},
		});
		await waitFor(() =>
			expect(area.textContent).toContain("/plan/attachments/image-1-0.png"),
		);
		expect(area.textContent).not.toContain("Attached image:");
		expect(area.textContent).not.toContain("read it with");
		expect(vi.mocked(saveAttachment)).toHaveBeenCalledTimes(1);
		expect(onInput).toHaveBeenCalled();
	});

	test("unsupported and oversize images never reach IPC", async () => {
		const onAttachmentError = vi.fn();
		const { area } = bubble({ onAttachmentError });
		setCaretAtEnd(area);
		const badType = new File(["<svg/>"], "vec.svg", { type: "image/svg+xml" });
		const big = pngFile("big.png");
		Object.defineProperty(big, "size", { value: MAX_BYTES + 1 });
		fireEvent.paste(area, {
			clipboardData: { getData: () => "", files: [badType], items: [] },
		});
		fireEvent.paste(area, {
			clipboardData: { getData: () => "", files: [big], items: [] },
		});
		await waitFor(() => expect(onAttachmentError).toHaveBeenCalledTimes(2));
		expect(vi.mocked(saveAttachment)).not.toHaveBeenCalled();
		expect(area.textContent).toBe("");
	});

	test("backend failure routes to onAttachmentError and inserts nothing", async () => {
		const onAttachmentError = vi.fn();
		const { area } = bubble({ onAttachmentError });
		setCaretAtEnd(area);
		vi.mocked(saveAttachment).mockRejectedValue(new Error("boom"));
		fireEvent.paste(area, {
			clipboardData: {
				getData: () => "",
				files: [pngFile()],
				items: [],
			},
		});
		await waitFor(() => expect(onAttachmentError).toHaveBeenCalledWith("boom"));
		expect(area.textContent).toBe("");
	});

	test("realistic typing stays sendable", async () => {
		const user = userEvent.setup();
		const { area, onSend } = bubble();
		await user.click(area);
		await user.keyboard("line one{Shift>}{Enter}{/Shift}line two{Enter}");
		expect(onSend).toHaveBeenCalledWith("line one\nline two");
	});

	test("mounts with initial text and caret at end", async () => {
		const user = userEvent.setup();
		const onSend = vi.fn();
		const { area } = bubble({ initialText: "TEMPLATE", onSend });
		expect(area.textContent).toBe("TEMPLATE");
		await user.keyboard("!{Enter}");
		expect(onSend).toHaveBeenCalledWith("TEMPLATE!");
		expect(area.textContent).toBe("");
	});

	test("reports edits through onInput", () => {
		const onInput = vi.fn();
		const { area } = bubble({ onInput });
		typeLines(area, "hello");
		fireEvent.input(area);
		expect(onInput).toHaveBeenCalledWith("hello");
	});

	test("late template fills an untouched bubble", () => {
		const onSend = vi.fn();
		const view = render(
			<DraftBubble
				initialText={undefined}
				session={testKey()}
				onSend={onSend}
				onEdit={vi.fn()}
				onInput={vi.fn()}
				onAttachmentError={vi.fn()}
			/>,
		);
		const area = screen.getByRole("textbox");
		expect(area.textContent).toBe("");
		view.rerender(
			<DraftBubble
				initialText="TEMPLATE"
				session={testKey()}
				onSend={onSend}
				onEdit={vi.fn()}
				onInput={vi.fn()}
				onAttachmentError={vi.fn()}
			/>,
		);
		expect(area.textContent).toBe("TEMPLATE");
	});

	test("late template never clobbers typed text", () => {
		const onSend = vi.fn();
		const onInput = vi.fn();
		const view = render(
			<DraftBubble
				initialText={undefined}
				session={testKey()}
				onSend={onSend}
				onEdit={vi.fn()}
				onInput={onInput}
				onAttachmentError={vi.fn()}
			/>,
		);
		const area = screen.getByRole("textbox");
		typeLines(area, "typed");
		fireEvent.input(area);
		view.rerender(
			<DraftBubble
				initialText="TEMPLATE"
				session={testKey()}
				onSend={onSend}
				onEdit={vi.fn()}
				onInput={onInput}
				onAttachmentError={vi.fn()}
			/>,
		);
		expect(area.textContent).toBe("typed");
	});
});
