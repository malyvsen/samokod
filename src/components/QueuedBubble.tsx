import { useEffect, useRef } from "react";
import type { QueuedMessage } from "../sessions/queue";
import {
	extractDraftText,
	insertPlainText,
	moveCaretToEnd,
	shouldCommitEnter,
} from "./editableText";

export function QueuedBubbleList({
	items,
	editingId,
	blockedId,
	onEdit,
	onCommit,
	onCancel,
}: {
	items: QueuedMessage[];
	editingId: string | null;
	blockedId: string | null;
	onEdit: (id: string) => void;
	onCommit: (id: string, text: string) => void;
	onCancel: () => void;
}) {
	if (items.length === 0) return null;
	return (
		<>
			{items.map((item) =>
				item.id === editingId ? (
					<QueuedBubbleEditor
						key={item.id}
						text={item.text}
						blocked={item.id === blockedId}
						onCommit={(text) => onCommit(item.id, text)}
						onCancel={onCancel}
					/>
				) : (
					<QueuedBubble
						key={item.id}
						text={item.text}
						onEdit={() => onEdit(item.id)}
					/>
				),
			)}
		</>
	);
}

function QueuedBubble({ text, onEdit }: { text: string; onEdit: () => void }) {
	return (
		// biome-ignore lint/a11y/useSemanticElements: queued edit is click-to-edit like the draft - a button element would bring native button metrics.
		<div
			className="queued"
			title="Click to edit"
			role="button"
			tabIndex={0}
			onClick={onEdit}
			onKeyDown={(event) => {
				if (event.key !== "Enter" && event.key !== " ") return;
				event.preventDefault();
				onEdit();
			}}
		>
			<div className="who-line">QUEUED</div>
			{text}
		</div>
	);
}

function QueuedBubbleEditor({
	text,
	blocked,
	onCommit,
	onCancel,
}: {
	text: string;
	blocked: boolean;
	onCommit: (text: string) => void;
	onCancel: () => void;
}) {
	const ref = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		const node = ref.current;
		if (node === null) return;
		node.textContent = text;
		node.focus();
		moveCaretToEnd(node);
	}, [text]);

	function commit(): void {
		const node = ref.current;
		if (node === null) {
			onCancel();
			return;
		}
		onCommit(extractDraftText(node));
	}

	return (
		<div className={blocked ? "queued blocked" : "queued"} title="Editing">
			<div className="who-line">QUEUED</div>
			{/* biome-ignore lint/a11y/useSemanticElements: contenteditable is the design - a textarea cannot size like a sent message without measuring code. */}
			<div
				ref={ref}
				className="edit"
				contentEditable
				tabIndex={0}
				role="textbox"
				aria-label="Edit queued message"
				onKeyDown={(event) => {
					if (event.key === "Escape") {
						event.preventDefault();
						onCancel();
						return;
					}
					if (!shouldCommitEnter(event)) return;
					event.preventDefault();
					commit();
				}}
				onPaste={(event) => {
					event.preventDefault();
					insertPlainText(event.clipboardData.getData("text/plain"));
				}}
				onBlur={onCancel}
			/>
		</div>
	);
}
