import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, test, vi } from "vitest";
import {
	executePlan,
	focusWindow,
	listOpenRepos,
	loadHistory,
	newWindow,
	onAppEvent,
	saveAttachment,
	selectPlan,
	setGlobalConfigOption,
	setPlanEvergreen,
	setPlanMode,
	warmSession,
} from "./api";
import type { AppEvent } from "./bindings";
import { testKey } from "./fixtures";

const listen = vi.hoisted(() => vi.fn());
const windowListen = vi.hoisted(() => vi.fn());

vi.mock("@tauri-apps/api/event", () => ({ listen }));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
	getCurrentWebviewWindow: () => ({ listen: windowListen }),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

type TauriHandler = (event: { payload: AppEvent }) => void;

function flush() {
	return new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
	listen.mockReset();
	windowListen.mockReset();
	vi.mocked(invoke).mockReset();
});

describe("session commands", () => {
	test("sends camelCase keys matching the Rust command", async () => {
		vi.mocked(invoke).mockResolvedValue([]);
		await setGlobalConfigOption("model", "openai/gpt-5");
		expect(invoke).toHaveBeenCalledWith("set_global_config_option", {
			configId: "model",
			value: "openai/gpt-5",
		});
	});

	test("select_plan targets the session", async () => {
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
			scoping_preview: null,
		});
		await selectPlan(testKey(), null);
		expect(invoke).toHaveBeenCalledWith("select_plan", {
			session: testKey(),
			draft: null,
		});
	});

	test("select_plan forwards the draft", async () => {
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
			scoping_preview: null,
		});
		await selectPlan(testKey(), "hello");
		expect(invoke).toHaveBeenCalledWith("select_plan", {
			session: testKey(),
			draft: "hello",
		});
	});

	test("execute_plan targets the session", async () => {
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
			scoping_preview: null,
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
			scoping_preview: null,
		});
		await setPlanMode("2026-09-25.10-54-59", true);
		expect(invoke).toHaveBeenCalledWith("set_plan_mode", {
			plan: "2026-09-25.10-54-59",
			manual: true,
		});
	});

	test("set_plan_evergreen targets the plan", async () => {
		vi.mocked(invoke).mockResolvedValue({
			plans: [],
			selected: testKey(),
			config_defaults: { model: null, effort: null },
			scoping_preview: null,
		});
		await setPlanEvergreen("2026-09-25.10-54-59", false);
		expect(invoke).toHaveBeenCalledWith("set_plan_evergreen", {
			plan: "2026-09-25.10-54-59",
			evergreen: false,
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

	test("save_attachment sends camelCase data", async () => {
		vi.mocked(invoke).mockResolvedValue("/abs/image-1-0.png");
		await saveAttachment(testKey(), "aGVsbG8=");
		expect(invoke).toHaveBeenCalledWith("save_attachment", {
			session: testKey(),
			dataBase64: "aGVsbG8=",
		});
	});

	test("list_open_repos calls the backend list", async () => {
		vi.mocked(invoke).mockResolvedValue([]);
		await listOpenRepos();
		expect(invoke).toHaveBeenCalledWith("list_open_repos");
	});

	test("focus_window targets the label", async () => {
		vi.mocked(invoke).mockResolvedValue(undefined);
		await focusWindow("window-1");
		expect(invoke).toHaveBeenCalledWith("focus_window", {
			label: "window-1",
		});
	});

	test("new_window opens a window", async () => {
		vi.mocked(invoke).mockResolvedValue("window-2");
		await expect(newWindow()).resolves.toBe("window-2");
		expect(invoke).toHaveBeenCalledWith("new_window");
	});
});

describe("app events", () => {
	test("unlistens when cleanup runs before listen resolves", async () => {
		const stop = vi.fn();
		let resolveWindow!: (stop: () => void) => void;
		let resolveGlobal!: (stop: () => void) => void;
		windowListen.mockReturnValueOnce(
			new Promise<() => void>((resolve) => {
				resolveWindow = resolve;
			}),
		);
		listen.mockReturnValueOnce(
			new Promise<() => void>((resolve) => {
				resolveGlobal = resolve;
			}),
		);

		const cleanup = onAppEvent(() => {});
		cleanup();
		resolveWindow(stop);
		resolveGlobal(stop);
		await flush();

		expect(stop).toHaveBeenCalledTimes(2);
	});

	test("quick resubscribe delivers each chunk once", async () => {
		const windowHandlers = new Set<TauriHandler>();
		const globalHandlers = new Set<TauriHandler>();
		windowListen.mockImplementation((_: unknown, handler: TauriHandler) => {
			windowHandlers.add(handler);
			return new Promise<() => void>((resolve) => {
				queueMicrotask(() => {
					resolve(() => {
						windowHandlers.delete(handler);
					});
				});
			});
		});
		listen.mockImplementation((_: unknown, handler: TauriHandler) => {
			globalHandlers.add(handler);
			return new Promise<() => void>((resolve) => {
				queueMicrotask(() => {
					resolve(() => {
						globalHandlers.delete(handler);
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

		expect(windowHandlers.size).toBe(1);
		expect(globalHandlers.size).toBe(1);
		for (const handler of [...windowHandlers]) {
			handler({
				payload: { type: "agent_text", session: testKey(), chunk: "Yes" },
			});
		}
		expect(seen).toEqual(["Yes"]);

		second();
		await flush();
		expect(windowHandlers.size).toBe(0);
		expect(globalHandlers.size).toBe(0);
	});

	test("window subscription takes session events, global takes only the broadcast", async () => {
		const windowHandlers = new Set<TauriHandler>();
		const globalHandlers = new Set<TauriHandler>();
		windowListen.mockImplementation((_: unknown, handler: TauriHandler) => {
			windowHandlers.add(handler);
			return Promise.resolve(() => {
				windowHandlers.delete(handler);
			});
		});
		listen.mockImplementation((_: unknown, handler: TauriHandler) => {
			globalHandlers.add(handler);
			return Promise.resolve(() => {
				globalHandlers.delete(handler);
			});
		});
		const seen: AppEvent[] = [];
		onAppEvent((event) => {
			seen.push(event);
		});
		await flush();

		const turn: AppEvent = { type: "turn_done", session: testKey() };
		const broadcast: AppEvent = { type: "open_repos_changed", repos: [] };
		for (const handler of [...windowHandlers]) {
			handler({ payload: turn });
			handler({ payload: broadcast });
		}
		for (const handler of [...globalHandlers]) {
			handler({ payload: turn });
			handler({ payload: broadcast });
		}
		// Session events arrive once via the window subscription; the
		// broadcast arrives once via the global subscription.
		expect(seen).toEqual([turn, broadcast]);
	});
});
