import { beforeEach, describe, expect, test, vi } from "vitest";
import { onAppEvent } from "./api";
import type { AppEvent } from "./types";

const listen = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

type TauriHandler = (event: { payload: AppEvent }) => void;

function flush() {
	return new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
	listen.mockReset();
});

describe("app events", () => {
	test("unlistens when cleanup runs before listen resolves", async () => {
		const stop = vi.fn();
		let resolveListen!: (stop: () => void) => void;
		listen.mockReturnValueOnce(
			new Promise<() => void>((resolve) => {
				resolveListen = resolve;
			}),
		);

		const cleanup = onAppEvent(() => {});
		cleanup();
		resolveListen(stop);
		await flush();

		expect(stop).toHaveBeenCalledTimes(1);
	});

	test("quick resubscribe delivers each chunk once", async () => {
		const handlers = new Set<TauriHandler>();
		listen.mockImplementation((_: unknown, handler: TauriHandler) => {
			handlers.add(handler);
			return new Promise<() => void>((resolve) => {
				queueMicrotask(() => {
					resolve(() => {
						handlers.delete(handler);
					});
				});
			});
		});
		const seen: string[] = [];
		const handle = (event: AppEvent) => {
			if (event.type === "agent_text") {
				seen.push(event.chunk);
			}
		};

		const first = onAppEvent(handle);
		first();
		const second = onAppEvent(handle);
		await flush();

		expect(handlers.size).toBe(1);
		for (const handler of [...handlers]) {
			handler({ payload: { type: "agent_text", chunk: "Yes" } });
		}
		expect(seen).toEqual(["Yes"]);

		second();
		await flush();
		expect(handlers.size).toBe(0);
	});
});
