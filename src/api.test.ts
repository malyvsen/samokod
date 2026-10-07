import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
	executePlan,
	loadHistory,
	onAppEvent,
	selectPlan,
	setConfigOption,
	setPlanMode,
	warmSession,
} from "./api";
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

describe("session commands", () => {
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
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
		});
		await selectPlan(testKey());
		expect(invoke).toHaveBeenCalledWith("select_plan", {
			session: testKey(),
		});
	});

	test("execute_plan targets the session", async () => {
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
		});
		await executePlan(testKey());
		expect(invoke).toHaveBeenCalledWith("execute_plan", {
			session: testKey(),
		});
	});

	test("set_plan_mode targets the plan", async () => {
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
		});
		await setPlanMode("2026-09-25.10-54-59", true);
		expect(invoke).toHaveBeenCalledWith("set_plan_mode", {
			plan: "2026-09-25.10-54-59",
			manual: true,
		});
	});

	test("warm_session targets the session", async () => {
		vi.mocked(invoke).mockResolvedValue(undefined);
		await warmSession(testKey());
		expect(invoke).toHaveBeenCalledWith("warm_session", {
			session: testKey(),
		});
	});

	test("load_history targets the session", async () => {
		vi.mocked(invoke).mockResolvedValue(undefined);
		await loadHistory(testKey());
		expect(invoke).toHaveBeenCalledWith("load_history", {
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
