import { useEffect, useRef } from "react";
import type { QueuedMessage, QueueMoveDirection } from "../sessions/queue";
import {
	extractText,
	insertPlainText,
	moveCaretToEnd,
	shouldCommitEnter,
} from "./editableText";
import { MessageMarkdown } from "./MessageMarkdown";

type QueuedHeaderControls =
	| { kind: "hidden" }
	| {
			kind: "movable";
			id: string;
			disableUp: boolean;
			disableDown: boolean;
			onMove: (id: string, direction: QueueMoveDirection) => void;
	  };

export function QueuedBubbleList({
	items,
	editingId,
	editingBlocked,
	onEdit,
	onCommit,
	onCancel,
	onMove,
}: {
	items: QueuedMessage[];
	editingId: string | null;
	editingBlocked: boolean;
	onEdit: (id: string) => void;
	onCommit: (id: string, text: string) => void;
	onCancel: () => void;
	onMove: (id: string, direction: QueueMoveDirection) => void;
}) {
	if (items.length === 0) return null;
	const showControls = items.length > 1;
	return (
		<>
			{items.map((item, index) => {
				const controls: QueuedHeaderControls =
					showControls === false
						? { kind: "hidden" }
						: {
								kind: "movable",
								id: item.id,
								disableUp: index === 0,
								disableDown: index === items.length - 1,
								onMove,
							};
				return item.id === editingId ? (
					<QueuedBubbleEditor
						key={item.id}
						text={item.text}
						blocked={editingBlocked}
						controls={controls}
						onCommit={(text) => onCommit(item.id, text)}
						onCancel={onCancel}
					/>
				) : (
					<QueuedBubble
						key={item.id}
						text={item.text}
						controls={controls}
						onEdit={() => onEdit(item.id)}
					/>
				);
			})}
		</>
	);
}

function QueuedBubble({
	text,
	controls,
	onEdit,
}: {
	text: string;
	controls: QueuedHeaderControls;
	onEdit: () => void;
}) {
	return (
		// biome-ignore lint/a11y/useSemanticElements: queued edit is click-to-edit like the draft - a button element would bring native button metrics.
		<div
			className="msg user queued"
			title="Click to edit"
			role="button"
			tabIndex={0}
			onClick={onEdit}
			onKeyDown={(event) => {
				if (event.target !== event.currentTarget) return;
				if (event.key !== "Enter" && event.key !== " ") return;
				event.preventDefault();
				onEdit();
			}}
		>
			<QueuedHeader controls={controls} />
			<div className="body">
				<MessageMarkdown text={text} />
			</div>
		</div>
	);
}

function QueuedBubbleEditor({
	text,
	blocked,
	controls,
	onCommit,
	onCancel,
}: {
	text: string;
	blocked: boolean;
	controls: QueuedHeaderControls;
	onCommit: (text: string) => void;
	onCancel: () => void;
}) {
	const containerRef = useRef<HTMLDivElement | null>(null);
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
		onCommit(extractText(node));
	}

	return (
		<div
			ref={containerRef}
			className={blocked ? "msg user queued blocked" : "msg user queued"}
			title="Editing"
		>
			<QueuedHeader controls={controls} />
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
				onBlur={(event) => {
					if (
						event.relatedTarget instanceof Node &&
						containerRef.current?.contains(event.relatedTarget) === true
					)
						return;
					onCancel();
				}}
			/>
		</div>
	);
}

function QueuedHeader({ controls }: { controls: QueuedHeaderControls }) {
	return (
		<div className="who">
			<span>QUEUED</span>
			{controls.kind === "movable" && (
				<QueuedMoveControls
					id={controls.id}
					disableUp={controls.disableUp}
					disableDown={controls.disableDown}
					onMove={controls.onMove}
				/>
			)}
		</div>
	);
}

function QueuedMoveControls({
	id,
	disableUp,
	disableDown,
	onMove,
}: {
	id: string;
	disableUp: boolean;
	disableDown: boolean;
	onMove: (id: string, direction: QueueMoveDirection) => void;
}) {
	return (
		<span className="qmove">
			<button
				type="button"
				aria-label="Move queued message up"
				disabled={disableUp}
				onMouseDown={(event) => event.preventDefault()}
				onClick={(event) => {
					event.stopPropagation();
					onMove(id, "up");
				}}
			>
				↑
			</button>
			<button
				type="button"
				aria-label="Move queued message down"
				disabled={disableDown}
				onMouseDown={(event) => event.preventDefault()}
				onClick={(event) => {
					event.stopPropagation();
					onMove(id, "down");
				}}
			>
				↓
			</button>
		</span>
	);
}
