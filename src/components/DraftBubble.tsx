import { useCallback, useEffect, useRef } from "react";
import {
	applyInitialText,
	extractText,
	insertPlainText,
	shouldCommitEnter,
} from "./editableText";

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
		const text = extractText(node);
		if (text === "") return;
		node.textContent = "";
		onSend(text);
	}

	function handleInput(): void {
		const node = ref.current;
		if (node !== null) {
			edited.current = true;
			onInput?.(extractText(node));
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
					if (!shouldCommitEnter(event)) return;
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
