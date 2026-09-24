import { useCallback, useRef } from "react";

const LABEL = "Ask for a change…";

export function DraftBubble({
	onSend,
	onEdit,
}: {
	onSend: (text: string) => void;
	onEdit: () => void;
}) {
	const ref = useRef<HTMLDivElement | null>(null);
	const attach = useCallback((node: HTMLDivElement | null) => {
		ref.current = node;
		node?.focus();
	}, []);

	function send(): void {
		const node = ref.current;
		if (node === null) return;
		const text = extractDraftText(node);
		if (text === "") return;
		node.textContent = "";
		onSend(text);
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
				aria-label={LABEL}
				data-placeholder={LABEL}
				onInput={onEdit}
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
					onEdit();
				}}
			/>
		</div>
	);
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

function collect(node: Node): string {
	let text = "";
	for (const child of node.childNodes) {
		if (child.nodeType === Node.TEXT_NODE) {
			text += child.nodeValue ?? "";
		} else if (child.nodeName === "BR") {
			text += "\n";
		} else if (child.nodeType === Node.ELEMENT_NODE) {
			text += collect(child);
			if (BLOCK_TAGS.has((child as Element).tagName)) text += "\n";
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
