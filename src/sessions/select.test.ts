import { describe, expect, test } from "vitest";
import { testEntryWith, testStatus } from "../fixtures";
import { isReadOnly } from "./select";

describe("isReadOnly", () => {
	test("finished phases are read-only", () => {
		for (const phase of ["completed", "cancelled"] as const) {
			const entry = testEntryWith("done", phase, "Done", true, [
				testStatus("scoping"),
			]);
			expect(isReadOnly(entry)).toBe(true);
		}
	});

	test("active phases stay writable", () => {
		for (const phase of ["scoping", "executing"] as const) {
			const entry = testEntryWith("run", phase, "Run", true, [
				testStatus("scoping"),
			]);
			expect(isReadOnly(entry)).toBe(false);
		}
	});

	test("no selection is writable", () => {
		expect(isReadOnly(undefined)).toBe(false);
	});
});
