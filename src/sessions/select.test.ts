import { describe, expect, test } from "vitest";
import { testEntryWith, testKey, testStatus } from "../fixtures";
import { sessionKeyOf } from "../types";
import {
	agentStatusOf,
	followRename,
	isReadOnly,
	isSessionBusy,
	repairSelection,
	selectedChat,
	sideContentFor,
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

describe("repairSelection", () => {
	test("keeps the current key when its plan still exists", () => {
		const plans = [
			testEntryWith("aaa", "scoping", "Alpha", true, [testStatus("scoping")]),
			testEntryWith("bbb", "scoping", "Beta", true, [testStatus("scoping")]),
		];
		const current = { plan: "aaa", role: "scoping" } as const;
		expect(repairSelection(plans, current)).toBe(current);
	});

	test("advances the role when the entry no longer lists it", () => {
		const plans = [
			testEntryWith("aaa", "executing", "Alpha", true, [
				testStatus("executing"),
			]),
		];
		expect(repairSelection(plans, { plan: "aaa", role: "scoping" })).toEqual({
			plan: "aaa",
			role: "executing",
		});
	});

	test("falls back to most-recent when the viewed plan vanished", () => {
		const plans = [
			testEntryWith("bbb", "scoping", "Beta", true, [testStatus("scoping")]),
		];
		expect(repairSelection(plans, { plan: "aaa", role: "scoping" })).toEqual({
			plan: "bbb",
			role: "scoping",
		});
	});

	test("returns null only for an empty list", () => {
		expect(repairSelection([], { plan: "aaa", role: "scoping" })).toBeNull();
		expect(repairSelection([], null)).toBeNull();
	});
});

describe("sideContentFor", () => {
	test("landing role returns commits", () => {
		const entry = testEntryWith(
			"a",
			"landing",
			"Alpha",
			true,
			[testStatus("scoping"), testStatus("landing")],
			false,
			true,
			["abc plan work", "def more work"],
			false,
		);
		expect(sideContentFor(entry, { plan: "a", role: "landing" }, [])).toEqual({
			kind: "commits",
			commits: ["abc plan work", "def more work"],
		});
	});

	test("other roles return todos", () => {
		const entry = testEntryWith(
			"a",
			"landing",
			"Alpha",
			true,
			[testStatus("scoping"), testStatus("executing")],
			false,
			true,
			["abc plan work"],
			false,
		);
		const todos = [{ content: "Wire view", status: "pending" } as const];
		expect(
			sideContentFor(entry, { plan: "a", role: "executing" }, [...todos]),
		).toEqual({ kind: "todos", todos: [...todos] });
		const executing = testEntryWith("b", "executing", "Beta", true, [
			testStatus("scoping"),
		]);
		expect(
			sideContentFor(executing, { plan: "b", role: "executing" }, [...todos]),
		).toEqual({ kind: "todos", todos: [...todos] });
		expect(sideContentFor(undefined, null, [...todos])).toEqual({
			kind: "todos",
			todos: [...todos],
		});
	});
});

describe("followRename", () => {
	test("honors selected when it derives from the acted plan", () => {
		const plans = [
			testEntryWith("aaa.slug", "executing", "Alpha", true, [
				testStatus("scoping"),
				testStatus("executing"),
			]),
		];
		expect(
			followRename(plans, "aaa", { plan: "aaa.slug", role: "executing" }),
		).toEqual({ plan: "aaa.slug", role: "executing" });
	});

	test("resolves the renamed entry otherwise", () => {
		const plans = [
			testEntryWith("aaa", "scoping", "Alpha", true, [testStatus("scoping")]),
			testEntryWith("bbb.slug", "executing", "Beta", true, [
				testStatus("scoping"),
				testStatus("executing"),
			]),
		];
		expect(
			followRename(plans, "bbb", { plan: "bbb.slug", role: "executing" }),
		).toEqual({ plan: "bbb.slug", role: "executing" });
	});

	test("falls back to most-recent when no rename matches", () => {
		const plans = [
			testEntryWith("ccc", "scoping", "Gamma", true, [testStatus("scoping")]),
		];
		expect(
			followRename(plans, "bbb", { plan: "zzz", role: "scoping" }),
		).toEqual({
			plan: "ccc",
			role: "scoping",
		});
	});
});
