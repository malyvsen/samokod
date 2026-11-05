import type { ClipboardEvent, DragEvent } from "react";
import { saveAttachment } from "../api";
import type { SessionKey } from "../bindings";
import { extractText, insertTextAtCaret } from "./editableText";

// Mirrors the backend `MAX_BYTES`: the client pre-validates so oversize
// images never reach IPC, the server still enforces.
export const MAX_BYTES = 8 * 1024 * 1024;

// Mirrors the backend magic-bytes allowlist. The client filters by MIME
// hint only; the server decides by content.
const ALLOWED_TYPES = new Set([
	"image/png",
	"image/jpeg",
	"image/webp",
	"image/gif",
]);

export function isSupportedImageFile(type: string, size: number): boolean {
	return ALLOWED_TYPES.has(type) && size <= MAX_BYTES;
}

export async function fileToBase64(file: File): Promise<string> {
	const buffer = await file.arrayBuffer();
	const bytes = new Uint8Array(buffer);
	let binary = "";
	const CHUNK = 0x8000;
	for (let offset = 0; offset < bytes.length; offset += CHUNK) {
		binary += String.fromCharCode(...bytes.subarray(offset, offset + CHUNK));
	}
	return btoa(binary);
}

// Attachment ref as the agent sees it: backticked repo-relative path.
// Backticks keep even a fallback absolute path from reading as a slash
// command. Pure.
export function attachmentRef(path: string): string {
	return `\`${path}\``;
}

// Separating spaces around a ref: exactly one where the neighbor holds
// non-whitespace, none where it holds whitespace or nothing (element
// boundary). Pure.
export function spacePadding(
	before: string | undefined,
	after: string | undefined,
): { prefix: string; suffix: string } {
	return {
		prefix: before !== undefined && !/\s/.test(before) ? " " : "",
		suffix: after !== undefined && !/\s/.test(after) ? " " : "",
	};
}

// Text immediately before/after the caret for space padding. The insert
// guards against carets outside the node, so no containment check here.
function caretPadding(): { prefix: string; suffix: string } {
	const selection = window.getSelection();
	if (selection === null || selection.rangeCount === 0)
		return spacePadding(undefined, undefined);
	const range = selection.getRangeAt(0);
	let before: string | undefined;
	let after: string | undefined;
	if (range.startContainer.nodeType === Node.TEXT_NODE) {
		before = (range.startContainer as Text).data[range.startOffset - 1];
	}
	if (range.endContainer.nodeType === Node.TEXT_NODE) {
		after = (range.endContainer as Text).data[range.endOffset];
	}
	return spacePadding(before, after);
}

async function handleImageFiles(
	node: HTMLElement,
	session: SessionKey,
	files: File[],
	options: {
		// Draft boxes sync the store on every insert; queued editors read
		// the DOM on commit, so they omit it.
		onInput?: ((text: string) => void) | undefined;
		onError: (message: string) => void;
	},
): Promise<void> {
	for (const file of files) {
		if (!isSupportedImageFile(file.type, file.size)) {
			options.onError(
				file.size > MAX_BYTES
					? `image too large: ${file.name || "pasted image"}`
					: `unsupported image type: ${file.type || "unknown"}`,
			);
			continue;
		}
		try {
			const base64 = await fileToBase64(file);
			const relativePath = await saveAttachment(session, base64);
			const { prefix, suffix } = caretPadding();
			insertTextAtCaret(
				node,
				`${prefix}${attachmentRef(relativePath)}${suffix}`,
			);
			options.onInput?.(extractText(node));
		} catch (error) {
			options.onError(error instanceof Error ? error.message : String(error));
		}
	}
}

function collectPasteImages(data: DataTransfer): File[] {
	const fromItems: File[] = [];
	if (data.items !== undefined) {
		for (const item of data.items) {
			if (item.kind === "file" && item.type.startsWith("image/")) {
				const file = item.getAsFile();
				if (file !== null) fromItems.push(file);
			}
		}
	}
	const seen = new Set(fromItems);
	for (const file of data.files) {
		if (file.type.startsWith("image/") && !seen.has(file)) {
			fromItems.push(file);
		}
	}
	return fromItems;
}

function collectDropImages(files: FileList | File[]): File[] {
	return [...files].filter((file) => file.type.startsWith("image/"));
}

/// Shared paste/drop wiring for the compose boxes. Images save and insert
/// a backticked repo-relative ref; imageless pastes fall through to
/// `onPlainText` so the draft and queued editors keep their own plain-text
/// sync.
export function attachmentHandlers(
	getNode: () => HTMLElement | null,
	session: SessionKey,
	sync: {
		onPlainText: (data: DataTransfer) => void;
		onInput?: ((text: string) => void) | undefined;
		onError: (message: string) => void;
	},
): {
	onPaste: (event: ClipboardEvent<HTMLElement>) => void;
	onDragOver: (event: DragEvent<HTMLElement>) => void;
	onDrop: (event: DragEvent<HTMLElement>) => void;
} {
	return {
		onPaste: (event) => {
			const images = collectPasteImages(event.clipboardData);
			if (images.length === 0) {
				event.preventDefault();
				sync.onPlainText(event.clipboardData);
				return;
			}
			event.preventDefault();
			const node = getNode();
			if (node === null) return;
			void handleImageFiles(node, session, images, {
				onInput: sync.onInput,
				onError: sync.onError,
			});
		},
		onDragOver: (event) => {
			event.preventDefault();
		},
		onDrop: (event) => {
			event.preventDefault();
			const images = collectDropImages(event.dataTransfer.files);
			if (images.length === 0) return;
			const node = getNode();
			if (node === null) return;
			void handleImageFiles(node, session, images, {
				onInput: sync.onInput,
				onError: sync.onError,
			});
		},
	};
}
