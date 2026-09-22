import { describe, expect, test } from "vitest";
import { greeting } from "./greeting";

describe("greeting", () => {
	test("addresses its subject", () => {
		expect(greeting("Tauri")).toBe("Hello, Tauri!");
	});
});
