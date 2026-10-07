import { useCallback, useEffect, useRef } from "react";

export function DraftBubble({
	initialText,
	placeholder = "Ask for a change…",
	onSend,
	onEdit,
	onInput,
}: {
	initialText?: string | undefined;
	placeholder?: string;
	onSend: (text: string) => void;
	onEdit: () => void;
	onInput?: ((text: string) => void) | undefined;
}) {
	const ref = useRef<HTMLDivElement | null>(null);
	const edited = useRef(false);
	const attach = useCallback((node: HTMLDivElement | null) => {
		ref.current = node;
		if (node === null) return;
		node.focus();
	}, []);

	useEffect(() => {
		applyInitialText(ref.current, initialText, edited.current);
	}, [initialText]);

	function send(): void {
		const node = ref.current;
		if (node === null) return;
		const text = extractDraftText(node);
		if (text === "") return;
		node.textContent = "";
		onSend(text);
	}

	function handleInput(): void {
		const node = ref.current;
		if (node !== null) {
			edited.current = true;
			onInput?.(extractDraftText(node));
		}
		onEdit();
	}

	return (
		<div className="msg user draft">
			<div className="who">YOU</div>
			{/* biome-ignore lint/a11y/useSemanticElements: contenteditable is the design - a textarea cannot size like a sent message without measuring code. */}
			<div
				ref={attach}
				className="edit"
				contentEditable
				tabIndex={0}
				role="textbox"
				aria-label={placeholder}
				data-placeholder={placeholder}
				onInput={handleInput}
				onKeyDown={(event) => {
					if (event.key !== "Enter") return;
					if (event.shiftKey) return;
					if (event.nativeEvent.isComposing) return;
					event.preventDefault();
					send();
				}}
				onPaste={(event) => {
					event.preventDefault();
					insertPlainText(event.clipboardData.getData("text/plain"));
					handleInput();
				}}
			/>
		</div>
	);
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
	const range = document.createRange();
	range.selectNodeContents(node);
	range.collapse(false);
	const selection = window.getSelection();
	selection?.removeAllRanges();
	selection?.addRange(range);
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

export function extractDraftText(root: HTMLElement): string {
	return collect(root).trim();
}

function insertPlainText(text: string): void {
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
