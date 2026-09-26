import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { onAppEvent, selectPlan, setConfigOption } from "./api";
import { testKey } from "./fixtures";
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
	vi.mocked(invoke).mockReset();
});

describe("config options", () => {
	test("sends camelCase keys matching the Rust command", async () => {
		vi.mocked(invoke).mockResolvedValue([]);
		await setConfigOption(testKey(), "model", "openai/gpt-5");
		expect(invoke).toHaveBeenCalledWith("set_config_option", {
			session: testKey(),
			configId: "model",
			value: "openai/gpt-5",
		});
	});

	test("select_plan targets the session", async () => {
		vi.mocked(invoke).mockResolvedValue({ plans: [], selected: testKey() });
		await selectPlan(testKey());
		expect(invoke).toHaveBeenCalledWith("select_plan", {
			session: testKey(),
		});
	});
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
			handler({
				payload: { type: "agent_text", session: testKey(), chunk: "Yes" },
			});
		}
		expect(seen).toEqual(["Yes"]);

		second();
		await flush();
		expect(handlers.size).toBe(0);
	});
});
