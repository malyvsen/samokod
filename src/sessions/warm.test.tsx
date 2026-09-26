import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { warmSession } from "../api";
import { testKey } from "../fixtures";
import { useWarmSession } from "./warm";

vi.mock("../api", () => ({ warmSession: vi.fn() }));

beforeEach(() => {
	vi.mocked(warmSession).mockReset();
	vi.mocked(warmSession).mockResolvedValue(undefined);
});

describe("useWarmSession", () => {
	test("fires once per no-live active key", () => {
		const { rerender } = renderHook(
			({ live }: { live: boolean }) => useWarmSession(testKey(), live, false),
			{ initialProps: { live: false } },
		);
		expect(warmSession).toHaveBeenCalledTimes(1);
		expect(warmSession).toHaveBeenCalledWith(testKey());
		rerender({ live: false });
		expect(warmSession).toHaveBeenCalledTimes(1);
		rerender({ live: true });
		expect(warmSession).toHaveBeenCalledTimes(1);
	});

	test("never fires for live keys", () => {
		renderHook(() => useWarmSession(testKey(), true, false));
		expect(warmSession).not.toHaveBeenCalled();
	});

	test("never fires for read-only keys", () => {
		renderHook(() => useWarmSession(testKey(), false, true));
		expect(warmSession).not.toHaveBeenCalled();
	});

	test("fires again for a new key", () => {
		const { rerender } = renderHook(
			({ plan }: { plan: string }) =>
				useWarmSession({ plan, role: "scoping" }, false, false),
			{ initialProps: { plan: "aaa" } },
		);
		expect(warmSession).toHaveBeenCalledTimes(1);
		rerender({ plan: "bbb" });
		expect(warmSession).toHaveBeenCalledTimes(2);
	});
});
