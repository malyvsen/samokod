import { describe, expect, test } from "vitest";
import { testEntryWith, testKey, testStatus } from "../fixtures";
import { sessionKeyOf } from "../types";
import {
	agentStatusOf,
	isReadOnly,
	isSessionBusy,
	selectedChat,
} from "./select";
import { emptyChat } from "./store";

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

describe("selectedChat", () => {
	test("missing entry is null", () => {
		expect(selectedChat({}, testKey())).toBeNull();
		expect(selectedChat({}, null)).toBeNull();
	});

	test("present entry returns the chat", () => {
		const chat = emptyChat();
		const chats = { [sessionKeyOf(testKey())]: chat };
		expect(selectedChat(chats, testKey())).toBe(chat);
	});
});

describe("isSessionBusy", () => {
	test("null is never busy", () => {
		expect(isSessionBusy(null)).toBe(false);
	});

	test("working and approval are busy", () => {
		expect(isSessionBusy({ ...emptyChat(), working: true })).toBe(true);
		expect(isSessionBusy({ ...emptyChat(), approval: true })).toBe(true);
	});

	test("replaying is busy but preparing is not", () => {
		expect(
			isSessionBusy({ ...emptyChat(), start: { kind: "replaying" } }),
		).toBe(true);
		expect(
			isSessionBusy({ ...emptyChat(), start: { kind: "preparing" } }),
		).toBe(false);
		expect(isSessionBusy(emptyChat())).toBe(false);
	});
});

describe("agentStatusOf", () => {
	test("null and start phases map to idle", () => {
		expect(agentStatusOf(null)).toBe("idle");
		for (const start of [
			{ kind: "idle" },
			{ kind: "preparing" },
			{ kind: "replaying" },
			{
				kind: "failed",
				error: { raw: "boom", hint: "hint", retryable: true },
			},
		] as const) {
			expect(agentStatusOf({ ...emptyChat(), start })).toBe("idle");
		}
	});

	test("live turn states still surface", () => {
		expect(agentStatusOf({ ...emptyChat(), approval: true })).toBe("approval");
		expect(agentStatusOf({ ...emptyChat(), working: true })).toBe("working");
		expect(agentStatusOf({ ...emptyChat(), failed: true })).toBe("failed");
	});
});
