import { saveAttachment } from "../api";
import type { SessionKey } from "../types";
import { extractText, insertTextAtCaret } from "./editableText";

export const MAX_BYTES = 8 * 1024 * 1024;

const ALLOWED_TYPES = new Set([
	"image/png",
	"image/jpeg",
	"image/webp",
	"image/gif",
]);

export function isSupportedImageFile(type: string, size: number): boolean {
	return ALLOWED_TYPES.has(type) && size <= MAX_BYTES;
}

export function insertedAttachmentText(abs: string): string {
	return `Attached image: ${abs} (read it with the read tool)`;
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

export async function handleImageFiles(
	node: HTMLElement,
	session: SessionKey,
	files: File[],
	options: {
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
			const abs = await saveAttachment(session, base64);
			insertTextAtCaret(node, `\n${insertedAttachmentText(abs)}\n`);
			options.onInput?.(extractText(node));
		} catch (error) {
			options.onError(error instanceof Error ? error.message : String(error));
		}
	}
}

export function collectPasteImages(data: DataTransfer | null): File[] {
	if (data === null) return [];
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
	for (const file of data.files ?? []) {
		if (file.type.startsWith("image/") && !seen.has(file)) {
			fromItems.push(file);
		}
	}
	return fromItems;
}

export function collectDropImages(files: FileList | File[] | null): File[] {
	if (files === null || files === undefined) return [];
	return [...files].filter((file) => file.type.startsWith("image/"));
}
