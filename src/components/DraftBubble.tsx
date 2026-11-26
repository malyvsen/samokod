import { useCallback, useEffect, useRef } from "react";
import type { SessionKey } from "../bindings";
import { formatContext } from "../spend";
import { attachmentHandlers } from "./attachments";
import {
	applyInitialText,
	extractText,
	insertPlainText,
	shouldCommitEnter,
} from "./editableText";

export function DraftBubble({
	initialText,
	placeholder = "Ask for a change…",
	session,
	contextPct,
	onSend,
	onEdit,
	onInput,
	onAttachmentError,
}: {
	initialText?: string | undefined;
	placeholder?: string;
	session: SessionKey;
	contextPct: number | null;
	onSend: (text: string) => void;
	onEdit: () => void;
	onInput?: ((text: string) => void) | undefined;
	onAttachmentError: (message: string) => void;
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

	function syncAfterAttachment(text: string): void {
		edited.current = true;
		onInput?.(text);
		onEdit();
	}

	const pasteDrop = attachmentHandlers(() => ref.current, session, {
		onPlainText: (data) => {
			insertPlainText(data.getData("text/plain"));
			handleInput();
		},
		onInput: syncAfterAttachment,
		onError: onAttachmentError,
	});

	return (
		<div className="msg user draft">
			<div className="who">
				<span>YOU</span>
				{contextPct === null ? null : (
					<span className="right">
						<span className="ctx-draft">{formatContext(contextPct)}</span>
					</span>
				)}
			</div>
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
				onPaste={pasteDrop.onPaste}
				onDragOver={pasteDrop.onDragOver}
				onDrop={pasteDrop.onDrop}
			/>
		</div>
	);
}
