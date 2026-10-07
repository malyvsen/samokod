import { act, fireEvent, render, screen } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { App } from "./App";
import { testDefaults, testEntry, testKey } from "./fixtures";
import type { AppEvent } from "./types";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const api = vi.hoisted(() => ({
	getPrefs: vi.fn(),
	validateRepo: vi.fn(),
	openRepo: vi.fn(),
	createPlan: vi.fn(),
	executePlan: vi.fn(),
	finishLanding: vi.fn(),
	beginLanding: vi.fn(),
	abandonPlan: vi.fn(),
	cancelExecution: vi.fn(),
	selectPlan: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setConfigOption: vi.fn(),
	warmSession: vi.fn(),
	loadHistory: vi.fn(),
	scopingTemplate: vi.fn(),
	onAppEvent: vi.fn(() => () => {}),
}));
vi.mock("./api", () => api);

beforeEach(() => {
	vi.clearAllMocks();
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		writable: true,
		value: () => ({
			matches: false,
			addEventListener: () => {},
			removeEventListener: () => {},
		}),
	});
	api.getPrefs.mockResolvedValue({
		recent: [{ path: "/repo" }],
	});
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	api.openRepo.mockResolvedValue({
		repo_root: "/repo",
		branch: "feature",
		plans: [testEntry()],
		selected: testKey(),
		config_defaults: testDefaults(),
	});
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.scopingTemplate.mockResolvedValue(null);
});

function emit(event: AppEvent) {
	const calls = api.onAppEvent.mock.calls as unknown as Array<
		[(event: AppEvent) => void]
	>;
	const call = calls.at(-1);
	if (call === undefined) throw new Error("no app event subscription");
	act(() => {
		call[0](event);
	});
}

async function openChat() {
	const user = userEvent.setup();
	render(<App />);
	await user.click(await screen.findByRole("button", { name: "open" }));
	await screen.findByRole("button", { name: "New plan" });
	return user;
}

function transcriptNode() {
	const node = document.querySelector(".transcript");
	if (!(node instanceof HTMLElement)) throw new Error("missing .transcript");
	return node;
}

function mockScroll(
	node: HTMLElement,
	dims: { scrollHeight: number; clientHeight: number; scrollTop: number },
) {
	let top = dims.scrollTop;
	Object.defineProperties(node, {
		scrollHeight: { configurable: true, get: () => dims.scrollHeight },
		clientHeight: { configurable: true, get: () => dims.clientHeight },
		scrollTop: {
			configurable: true,
			get: () => top,
			set: (value: number) => {
				top = value;
			},
		},
	});
}

describe("transcript scroll wiring", () => {
	test("pinned transcript auto-scrolls on streaming chunks", async () => {
		await openChat();
		const node = transcriptNode();
		mockScroll(node, { scrollHeight: 1000, clientHeight: 500, scrollTop: 500 });
		fireEvent.scroll(node);
		emit({ type: "agent_text", session: testKey(), chunk: "hello" });
		expect(node.scrollTop).toBe(1000);
		emit({
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "edit file.md", status: "completed" },
		});
		expect(node.scrollTop).toBe(1000);
	});

	test("unpinned transcript holds position on streaming chunks", async () => {
		await openChat();
		const node = transcriptNode();
		mockScroll(node, { scrollHeight: 1000, clientHeight: 500, scrollTop: 0 });
		fireEvent.scroll(node);
		emit({ type: "agent_text", session: testKey(), chunk: "hello" });
		expect(node.scrollTop).toBe(0);
		emit({
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "edit file.md", status: "completed" },
		});
		expect(node.scrollTop).toBe(0);
	});
});
