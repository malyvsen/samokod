import { describe, expect, test } from "vitest";
import {
	applyInitialText,
	extractText,
	insertTextAtCaret,
} from "./components/editableText";

describe("applyInitialText", () => {
	function nodeWith(text: string) {
		const node = document.createElement("div");
		node.textContent = text;
		return node;
	}

	test("applies when untouched", () => {
		const node = nodeWith("");
		applyInitialText(node, "TEMPLATE", false);
		expect(node.textContent).toBe("TEMPLATE");
	});

	test("skips when edited", () => {
		const node = nodeWith("typed");
		applyInitialText(node, "TEMPLATE", true);
		expect(node.textContent).toBe("typed");
	});

	test("skips when already matching", () => {
		const node = nodeWith("TEMPLATE");
		applyInitialText(node, "TEMPLATE", false);
		expect(node.textContent).toBe("TEMPLATE");
	});
});

describe("extractText", () => {
	function root(html: string) {
		const node = document.createElement("div");
		node.innerHTML = html;
		return node;
	}

	test("keeps internal newlines, trims the ends", () => {
		expect(extractText(root("a\nb"))).toBe("a\nb");
		expect(extractText(root("\n  a\nb  \n"))).toBe("a\nb");
	});

	test("turns pasted blocks into lines", () => {
		expect(extractText(root("<div>a</div><div>b</div>"))).toBe("a\nb");
		expect(extractText(root("a<br>b"))).toBe("a\nb");
		expect(extractText(root("a<br>"))).toBe("a");
	});

	test("blank edits read as empty", () => {
		expect(extractText(root(""))).toBe("");
		expect(extractText(root("<br>"))).toBe("");
		expect(extractText(root("<div><br></div>"))).toBe("");
	});

	test("does not glue bare text to blocks (firstsecond bug)", () => {
		expect(extractText(root("first<div>second</div><div>third</div>"))).toBe(
			"first\nsecond\nthird",
		);
	});

	test("keeps nested breaks after bare text (firstsecond bug)", () => {
		expect(extractText(root("first<div>second<br>third</div>"))).toBe(
			"first\nsecond\nthird",
		);
	});

	test("preserves blank lines between blocks (no collapsing)", () => {
		expect(extractText(root("a<div><br></div><div>b</div>"))).toBe("a\n\nb");
	});
});

describe("insertTextAtCaret", () => {
	test("inserts mid-text and collapses after", () => {
		const node = document.createElement("div");
		node.textContent = "helloworld";
		document.body.appendChild(node);
		try {
			const text = node.firstChild;
			if (text === null) throw new Error("no text node");
			const range = document.createRange();
			range.setStart(text, 5);
			range.collapse(true);
			const selection = window.getSelection();
			selection?.removeAllRanges();
			selection?.addRange(range);
			insertTextAtCaret(node, " ");
			expect(node.textContent).toBe("hello world");
		} finally {
			node.remove();
		}
	});

	test("ignores a caret outside the root", () => {
		const root = document.createElement("div");
		root.textContent = "inside";
		const outside = document.createElement("div");
		outside.textContent = "outside";
		document.body.append(root, outside);
		try {
			const text = outside.firstChild;
			if (text === null) throw new Error("no text node");
			const range = document.createRange();
			range.setStart(text, 1);
			range.collapse(true);
			const selection = window.getSelection();
			selection?.removeAllRanges();
			selection?.addRange(range);
			insertTextAtCaret(root, "X");
			expect(root.textContent).toBe("inside");
		} finally {
			root.remove();
			outside.remove();
		}
	});
});
