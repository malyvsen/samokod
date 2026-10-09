import { describe, expect, test } from "vitest";
import { testKey } from "../fixtures";
import { type AppEvent, sessionKeyOf } from "../types";
import {
	applySessionEvent,
	type ChatState,
	type Chats,
	emptyChat,
	updateEntry,
} from "./store";

const KEY_ID = sessionKeyOf(testKey());

function withChat(): Chats {
	return updateEntry({}, testKey(), (chat) => chat);
}

function eventFor(event: AppEvent): Chats {
	return applySessionEvent(withChat(), event);
}

function chatOf(chats: Chats): ChatState {
	return chats[KEY_ID] ?? emptyChat();
}

describe("history events", () => {
	test("preparing marks preparing", () => {
		const chats = eventFor({ type: "history_preparing", session: testKey() });
		const chat = chatOf(chats);
		expect(chat.start).toEqual({ kind: "preparing" });
	});

	test("begin marks replaying and clears a past error", () => {
		const failed = applySessionEvent(withChat(), {
			type: "history_failed",
			session: testKey(),
			raw: "boom",
			hint: "retry the turn",
			retryable: true,
		});
		const begun = applySessionEvent(failed, {
			type: "history_begin",
			session: testKey(),
		});
		const chat = chatOf(begun);
		expect(chat.start).toEqual({ kind: "replaying" });
	});

	test("preparing retry clears the partial replay", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "agent_text",
			session: testKey(),
			chunk: "partial",
		});
		chats = applySessionEvent(chats, {
			type: "history_failed",
			session: testKey(),
			raw: "boom",
			hint: "retry the turn",
			retryable: true,
		});
		chats = applySessionEvent(chats, {
			type: "history_preparing",
			session: testKey(),
		});
		const chat = chatOf(chats);
		expect(chat.transcript).toEqual([]);
		expect(chat.start).toEqual({ kind: "preparing" });
	});

	test("retry begin clears the partial replay", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "agent_text",
			session: testKey(),
			chunk: "partial",
		});
		chats = applySessionEvent(chats, {
			type: "history_failed",
			session: testKey(),
			raw: "boom",
			hint: "retry the turn",
			retryable: true,
		});
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		const chat = chatOf(chats);
		expect(chat.transcript).toEqual([]);
	});

	test("begin never clobbers a live transcript", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_done",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "user_text",
			session: testKey(),
			chunk: "live",
		});
		const chat = chatOf(chats);
		expect(chat.transcript).toHaveLength(1);
	});

	test("done resets to idle and keeps the replay", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "user_text",
			session: testKey(),
			chunk: "hello",
		});
		chats = applySessionEvent(chats, {
			type: "history_done",
			session: testKey(),
		});
		const chat = chatOf(chats);
		expect(chat.start).toEqual({ kind: "idle" });
		expect(chat.transcript).toHaveLength(1);
	});

	test("failed keeps the partial replay with an error", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "user_text",
			session: testKey(),
			chunk: "partial",
		});
		chats = applySessionEvent(chats, {
			type: "history_failed",
			session: testKey(),
			raw: "boom",
			hint: "history hint",
			retryable: false,
		});
		const chat = chatOf(chats);
		expect(chat.start).toEqual({
			kind: "failed",
			error: { raw: "boom", hint: "history hint", retryable: false },
		});
		expect(chat.transcript).toHaveLength(1);
	});
});

describe("scoping preview", () => {
	test("empty chat starts with no preview", () => {
		expect(emptyChat().scopingPreview).toBeNull();
	});

	test("history_begin clears scopingPreview while keeping the transcript", () => {
		let chats = updateEntry(withChat(), testKey(), (chat) => ({
			...chat,
			scopingPreview: "TEMPLATE",
			transcript: [{ kind: "user", id: "u1", text: "hello" }],
		}));
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		const chat = chatOf(chats);
		expect(chat.scopingPreview).toBeNull();
		expect(chat.transcript).toHaveLength(1);
		expect(chat.transcript[0]).toMatchObject({ kind: "user", text: "hello" });
	});
});

describe("replayed updates", () => {
	test("user messages stay as separate bubbles without touching working", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "user_text",
			session: testKey(),
			chunk: "hel",
		});
		chats = applySessionEvent(chats, {
			type: "user_text",
			session: testKey(),
			chunk: "lo",
		});
		const chat = chatOf(chats);
		expect(chat.transcript).toHaveLength(2);
		expect(chat.transcript[0]).toMatchObject({ kind: "user", text: "hel" });
		expect(chat.transcript[1]).toMatchObject({ kind: "user", text: "lo" });
		expect(chat.working).toBe(false);
	});

	test("agent and tool replay leaves working false", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "agent_text",
			session: testKey(),
			chunk: "hello",
		});
		chats = applySessionEvent(chats, {
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "edit file.md", status: "completed" },
		});
		const chat = chatOf(chats);
		expect(chat.transcript).toHaveLength(2);
		expect(chat.working).toBe(false);
		expect(chat.failed).toBe(false);
	});

	test("replayed approvals resolve without pausing", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(chats, {
			type: "permission_asked",
			session: testKey(),
			permission: {
				tool_call_id: "tc1",
				title: "run this action?",
				tool: "bash",
				options: [{ id: "a", kind: "allow" }],
				rule_hint: "hint",
			},
		});
		const chat = chatOf(chats);
		expect(chat.approval).toBe(false);
		expect(chat.transcript).toHaveLength(1);
		expect(chat.transcript[0]).toMatchObject({
			kind: "approval",
			resolved: true,
		});
	});

	test("live turns still mark working and pause", () => {
		let chats = eventFor({
			type: "agent_text",
			session: testKey(),
			chunk: "hello",
		});
		const working = chatOf(chats);
		expect(working.working).toBe(true);
		chats = applySessionEvent(chats, {
			type: "permission_asked",
			session: testKey(),
			permission: {
				tool_call_id: "tc1",
				title: "run this action?",
				tool: "bash",
				options: [{ id: "a", kind: "allow" }],
				rule_hint: "hint",
			},
		});
		const paused = chatOf(chats);
		expect(paused.approval).toBe(true);
		expect(paused.transcript[1]).toMatchObject({
			kind: "approval",
			resolved: false,
		});
	});
});

describe("thought bursts", () => {
	test("thoughts stay silent while replaying", () => {
		let chats = eventFor({ type: "history_preparing", session: testKey() });
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		chats = applySessionEvent(
			chats,
			{ type: "agent_thought", session: testKey(), chunk: "hmm" },
			1000,
		);
		const chat = chatOf(chats);
		expect(chat.live).toBeNull();
		expect(chat.transcript).toEqual([]);
		expect(chat.working).toBe(false);
	});

	test("content freezes the burst ahead with seconds", () => {
		let chats = applySessionEvent(
			withChat(),
			{ type: "agent_thought", session: testKey(), chunk: "hello" },
			1000,
		);
		chats = applySessionEvent(
			chats,
			{ type: "agent_text", session: testKey(), chunk: "answer" },
			2000,
		);
		const chat = chatOf(chats);
		expect(chat.transcript[0]).toMatchObject({ kind: "thought", seconds: 1 });
		expect(chat.transcript[1]).toMatchObject({ kind: "agent", text: "answer" });
		expect(chat.live).toEqual({ kind: "waiting" });
	});

	test("turn end freezes the burst and clears live", () => {
		let chats = applySessionEvent(
			withChat(),
			{ type: "agent_thought", session: testKey(), chunk: "hello" },
			1000,
		);
		chats = applySessionEvent(
			chats,
			{ type: "turn_done", session: testKey() },
			9000,
		);
		const chat = chatOf(chats);
		expect(chat.transcript[0]).toMatchObject({ kind: "thought", seconds: 1 });
		expect(chat.live).toBeNull();
		expect(chat.working).toBe(false);
	});

	test("history begin freezes the burst and stays silent", () => {
		let chats = applySessionEvent(
			withChat(),
			{ type: "agent_thought", session: testKey(), chunk: "hello" },
			1000,
		);
		chats = applySessionEvent(chats, {
			type: "history_begin",
			session: testKey(),
		});
		const chat = chatOf(chats);
		expect(chat.transcript[0]).toMatchObject({ kind: "thought", seconds: 1 });
		expect(chat.live).toBeNull();
		expect(chat.start).toEqual({ kind: "replaying" });
	});
});
