import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { warmSession } from "../api";
import { testKey } from "../fixtures";
import { sessionKeyOf } from "../types";
import type { Chats } from "./store";
import { emptyChat } from "./store";
import { useWarmSession } from "./warm";

vi.mock("../api", () => ({ warmSession: vi.fn() }));

beforeEach(() => {
	vi.mocked(warmSession).mockReset();
	vi.mocked(warmSession).mockResolvedValue(undefined);
});

const KEY_ID = sessionKeyOf(testKey());
const liveChats = (chats: Chats) => ({ chats });

function readyChats(): Chats {
	return {
		[KEY_ID]: {
			...emptyChat(),
			transcript: [{ kind: "user", id: "1", text: "hi" }],
		},
	};
}

describe("useWarmSession", () => {
	test("fires once per idle non-empty key", () => {
		const { rerender } = renderHook(
			({ chats }: { chats: Chats }) => useWarmSession(testKey(), chats, false),
			{ initialProps: liveChats(readyChats()) },
		);
		expect(warmSession).toHaveBeenCalledTimes(1);
		expect(warmSession).toHaveBeenCalledWith(testKey());
		rerender(liveChats(readyChats()));
		expect(warmSession).toHaveBeenCalledTimes(1);
	});

	test("never fires for live, empty, or working keys", () => {
		const live: Chats = {
			[KEY_ID]: {
				...emptyChat(),
				transcript: [{ kind: "user", id: "1", text: "hi" }],
				configOptions: [
					{
						id: "model",
						name: "Model",
						category: "model",
						currentValue: "m",
						options: [],
					},
				],
			},
		};
		renderHook(() => useWarmSession(testKey(), live, false));
		expect(warmSession).not.toHaveBeenCalled();
		renderHook(() => useWarmSession(testKey(), {}, false));
		expect(warmSession).not.toHaveBeenCalled();
		const working: Chats = {
			[KEY_ID]: {
				...emptyChat(),
				transcript: [{ kind: "user", id: "1", text: "hi" }],
				working: true,
			},
		};
		renderHook(() => useWarmSession(testKey(), working, false));
		expect(warmSession).not.toHaveBeenCalled();
	});

	test("never fires for read-only keys", () => {
		renderHook(() => useWarmSession(testKey(), readyChats(), true));
		expect(warmSession).not.toHaveBeenCalled();
	});

	test("fires again for a new key", () => {
		const { rerender } = renderHook(
			({ plan }: { plan: string }) =>
				useWarmSession(
					{ plan, role: "scoping" },
					{
						[sessionKeyOf({ plan, role: "scoping" })]: {
							...emptyChat(),
							transcript: [{ kind: "user", id: "1", text: "hi" }],
						},
					},
					false,
				),
			{ initialProps: { plan: "aaa" } },
		);
		expect(warmSession).toHaveBeenCalledTimes(1);
		rerender({ plan: "bbb" });
		expect(warmSession).toHaveBeenCalledTimes(2);
	});
});
