import { describe, expect, test } from "vitest";
import {
	fileToBase64,
	insertedAttachmentText,
	isSupportedImageFile,
	MAX_BYTES,
} from "./components/attachments";

describe("attachments", () => {
	test("inserted text names the path for the read tool", () => {
		expect(insertedAttachmentText("/abs/image-1-0.png")).toBe(
			"Attached image: /abs/image-1-0.png (read it with the read tool)",
		);
	});

	test("filters by allowlist and size", () => {
		for (const type of ["image/png", "image/jpeg", "image/webp", "image/gif"]) {
			expect(isSupportedImageFile(type, 100)).toBe(true);
		}
		expect(isSupportedImageFile("text/plain", 100)).toBe(false);
		expect(isSupportedImageFile("image/png", MAX_BYTES + 1)).toBe(false);
		expect(isSupportedImageFile("image/svg+xml", 100)).toBe(false);
	});

	test("fileToBase64 round-trips bytes", async () => {
		const bytes = new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]);
		const file = new File([bytes], "shot.png", { type: "image/png" });
		(
			file as unknown as { arrayBuffer: () => Promise<ArrayBuffer> }
		).arrayBuffer = async () => bytes.buffer as ArrayBuffer;
		const encoded = await fileToBase64(file);
		const decoded = Uint8Array.from(atob(encoded), (char) =>
			char.charCodeAt(0),
		);
		expect(decoded).toEqual(bytes);
	});
});
