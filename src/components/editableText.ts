export function extractText(root: HTMLElement): string {
	return collect(root).trim();
}

export function insertPlainText(text: string): void {
	const selection = window.getSelection();
	if (selection === null || selection.rangeCount === 0) return;
	const range = selection.getRangeAt(0);
	range.deleteContents();
	const node = document.createTextNode(text);
	range.insertNode(node);
	range.setStartAfter(node);
	range.collapse(true);
	selection.removeAllRanges();
	selection.addRange(range);
}

export function shouldCommitEnter(event: {
	key: string;
	shiftKey: boolean;
	nativeEvent: { isComposing: boolean };
}): boolean {
	if (event.key !== "Enter") return false;
	if (event.shiftKey) return false;
	if (event.nativeEvent.isComposing) return false;
	return true;
}

export function moveCaretToEnd(node: HTMLElement): void {
	const range = document.createRange();
	range.selectNodeContents(node);
	range.collapse(false);
	const selection = window.getSelection();
	selection?.removeAllRanges();
	selection?.addRange(range);
}

export function applyInitialText(
	node: HTMLElement | null,
	initialText: string | undefined,
	edited: boolean,
): void {
	if (node === null || edited) return;
	const text = initialText ?? "";
	if ((node.textContent ?? "") === text) return;
	node.textContent = text;
	if (text === "") return;
	moveCaretToEnd(node);
}

const BLOCK_TAGS = new Set([
	"ADDRESS",
	"ARTICLE",
	"ASIDE",
	"BLOCKQUOTE",
	"DD",
	"DETAILS",
	"DIALOG",
	"DIV",
	"DL",
	"DT",
	"FIELDSET",
	"FIGCAPTION",
	"FIGURE",
	"FOOTER",
	"FORM",
	"H1",
	"H2",
	"H3",
	"H4",
	"H5",
	"H6",
	"HEADER",
	"HGROUP",
	"HR",
	"LI",
	"MAIN",
	"NAV",
	"OL",
	"P",
	"PRE",
	"SECTION",
	"TABLE",
	"UL",
]);

function isBlock(element: Element): boolean {
	return BLOCK_TAGS.has(element.tagName);
}

function collect(node: Node): string {
	let text = "";
	for (const child of node.childNodes) {
		if (child.nodeType === Node.TEXT_NODE) {
			text += child.nodeValue ?? "";
		} else if (child.nodeName === "BR") {
			text += "\n";
		} else if (child.nodeType === Node.ELEMENT_NODE) {
			if (isBlock(child as Element)) {
				if (text !== "" && !text.endsWith("\n")) text += "\n";
			}
			text += collect(child);
		}
	}
	return text;
}
