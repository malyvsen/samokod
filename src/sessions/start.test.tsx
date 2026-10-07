import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { loadHistory, warmSession } from "../api";
import { testKey } from "../fixtures";
import { sessionKeyOf } from "../types";
import { useSessionStart } from "./start";
import { type Chats, emptyChat } from "./store";

vi.mock("../api", () => ({ loadHistory: vi.fn(), warmSession: vi.fn() }));

beforeEach(() => {
	vi.mocked(loadHistory).mockReset();
	vi.mocked(loadHistory).mockResolvedValue(undefined);
	vi.mocked(warmSession).mockReset();
	vi.mocked(warmSession).mockResolvedValue(undefined);
});

const KEY_ID = sessionKeyOf(testKey());

function idleEmpty(): Chats {
	return { [KEY_ID]: emptyChat() };
}

function idleNonEmpty(): Chats {
	return {
		[KEY_ID]: {
			...emptyChat(),
			transcript: [{ kind: "user", id: "1", text: "hi" }],
		},
	};
}

describe("useSessionStart", () => {
	test("unknown entry fires loadHistory once", () => {
		const { rerender } = renderHook(
			({ chats }: { chats: Chats }) => useSessionStart(testKey(), chats, false),
			{ initialProps: { chats: {} } },
		);
		expect(loadHistory).toHaveBeenCalledTimes(1);
		expect(loadHistory).toHaveBeenCalledWith(testKey());
		expect(warmSession).not.toHaveBeenCalled();
		rerender({ chats: {} });
		expect(loadHistory).toHaveBeenCalledTimes(1);
	});

	test("empty transcript fires loadHistory once", () => {
		const { rerender } = renderHook(
			({ chats }: { chats: Chats }) => useSessionStart(testKey(), chats, false),
			{ initialProps: { chats: idleEmpty() } },
		);
		expect(loadHistory).toHaveBeenCalledTimes(1);
		expect(warmSession).not.toHaveBeenCalled();
		rerender({ chats: idleEmpty() });
		expect(loadHistory).toHaveBeenCalledTimes(1);
	});

	test("non-empty idle fires warmSession once", () => {
		const { rerender } = renderHook(
			({ chats }: { chats: Chats }) => useSessionStart(testKey(), chats, false),
			{ initialProps: { chats: idleNonEmpty() } },
		);
		expect(warmSession).toHaveBeenCalledTimes(1);
		expect(warmSession).toHaveBeenCalledWith(testKey());
		expect(loadHistory).not.toHaveBeenCalled();
		rerender({ chats: idleNonEmpty() });
		expect(warmSession).toHaveBeenCalledTimes(1);
	});

	test("live, working, and read-only fire neither", () => {
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
		renderHook(() => useSessionStart(testKey(), live, false));
		expect(loadHistory).not.toHaveBeenCalled();
		expect(warmSession).not.toHaveBeenCalled();

		const working: Chats = {
			[KEY_ID]: { ...emptyChat(), working: true },
		};
		renderHook(() => useSessionStart(testKey(), working, false));
		expect(loadHistory).not.toHaveBeenCalled();
		expect(warmSession).not.toHaveBeenCalled();

		renderHook(() => useSessionStart(testKey(), idleNonEmpty(), true));
		expect(loadHistory).not.toHaveBeenCalled();
		expect(warmSession).not.toHaveBeenCalled();

		renderHook(() => useSessionStart(null, idleEmpty(), false));
		expect(loadHistory).not.toHaveBeenCalled();
		expect(warmSession).not.toHaveBeenCalled();
	});

	test("fires again for a new key", () => {
		const { rerender } = renderHook(
			({ plan }: { plan: string }) =>
				useSessionStart({ plan, role: "scoping" }, {}, false),
			{ initialProps: { plan: "aaa" } },
		);
		expect(loadHistory).toHaveBeenCalledTimes(1);
		rerender({ plan: "bbb" });
		expect(loadHistory).toHaveBeenCalledTimes(2);
	});
});
