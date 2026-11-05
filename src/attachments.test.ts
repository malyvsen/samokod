import { describe, expect, test } from "vitest";
import {
	attachmentRef,
	fileToBase64,
	isSupportedImageFile,
	MAX_BYTES,
	spacePadding,
} from "./components/attachments";

describe("attachments", () => {
	test("ref wraps the path in backticks", () => {
		expect(
			attachmentRef(".samokod/plans/scoping/test/attachments/image-1-0.png"),
		).toBe("`.samokod/plans/scoping/test/attachments/image-1-0.png`");
	});

	test("padding separates only from non-whitespace", () => {
		expect(spacePadding("o", "w")).toEqual({ prefix: " ", suffix: " " });
		expect(spacePadding(" ", " ")).toEqual({ prefix: "", suffix: "" });
		expect(spacePadding(" ", "w")).toEqual({ prefix: "", suffix: " " });
		expect(spacePadding("o", " ")).toEqual({ prefix: " ", suffix: "" });
		expect(spacePadding(undefined, undefined)).toEqual({
			prefix: "",
			suffix: "",
		});
		expect(spacePadding("\n", "\t")).toEqual({ prefix: "", suffix: "" });
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
