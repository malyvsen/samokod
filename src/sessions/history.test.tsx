import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { loadHistory } from "../api";
import { testKey } from "../fixtures";
import { sessionKeyOf } from "../types";
import { useSessionHistory } from "./history";
import { type Chats, emptyChat } from "./store";

vi.mock("../api", () => ({ loadHistory: vi.fn() }));

beforeEach(() => {
	vi.mocked(loadHistory).mockReset();
	vi.mocked(loadHistory).mockResolvedValue(undefined);
});

const KEY_ID = sessionKeyOf(testKey());

describe("useSessionHistory", () => {
	test("fires once per empty-transcript key", () => {
		const { rerender } = renderHook(
			({ chats }: { chats: Chats }) => useSessionHistory(testKey(), chats),
			{ initialProps: { chats: {} } },
		);
		expect(loadHistory).toHaveBeenCalledTimes(1);
		expect(loadHistory).toHaveBeenCalledWith(testKey());
		rerender({ chats: {} });
		expect(loadHistory).toHaveBeenCalledTimes(1);
	});

	test("never fires for keys with live messages", () => {
		const chats: Chats = {
			[KEY_ID]: {
				...emptyChat(),
				transcript: [{ kind: "user", id: "1", text: "hi" }],
			},
		};
		renderHook(() => useSessionHistory(testKey(), chats));
		expect(loadHistory).not.toHaveBeenCalled();
	});

	test("never fires while loading or failed", () => {
		const loading: Chats = {
			[KEY_ID]: { ...emptyChat(), historyLoading: true },
		};
		renderHook(() => useSessionHistory(testKey(), loading));
		expect(loadHistory).not.toHaveBeenCalled();
		const failed: Chats = {
			[KEY_ID]: { ...emptyChat(), historyError: "boom" },
		};
		renderHook(() => useSessionHistory(testKey(), failed));
		expect(loadHistory).not.toHaveBeenCalled();
	});

	test("fires again for a new key", () => {
		const { rerender } = renderHook(
			({ plan }: { plan: string }) =>
				useSessionHistory({ plan, role: "scoping" }, {}),
			{ initialProps: { plan: "aaa" } },
		);
		expect(loadHistory).toHaveBeenCalledTimes(1);
		rerender({ plan: "bbb" });
		expect(loadHistory).toHaveBeenCalledTimes(2);
	});
});
