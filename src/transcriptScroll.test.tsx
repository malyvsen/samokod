import { fireEvent } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { testDefaults, testEntry, testKey } from "./fixtures";
import { api, emitAppEvent, openChat, stubMatchMedia } from "./testHarness";

vi.mock("./api", async () => {
	const { api } = await import("./testHarness");
	return api;
});

vi.mock("@tauri-apps/plugin-dialog", async () => {
	const { dialog } = await import("./testHarness");
	return dialog;
});

beforeEach(() => {
	vi.clearAllMocks();
	stubMatchMedia();
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
		await openChat(api);
		const node = transcriptNode();
		mockScroll(node, { scrollHeight: 1000, clientHeight: 500, scrollTop: 500 });
		fireEvent.scroll(node);
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "hello",
		});
		expect(node.scrollTop).toBe(1000);
		emitAppEvent(api, {
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "edit file.md", status: "completed" },
		});
		expect(node.scrollTop).toBe(1000);
	});

	test("unpinned transcript holds position on streaming chunks", async () => {
		await openChat(api);
		const node = transcriptNode();
		mockScroll(node, { scrollHeight: 1000, clientHeight: 500, scrollTop: 0 });
		fireEvent.scroll(node);
		emitAppEvent(api, {
			type: "agent_text",
			session: testKey(),
			chunk: "hello",
		});
		expect(node.scrollTop).toBe(0);
		emitAppEvent(api, {
			type: "tool_line",
			session: testKey(),
			line: { id: "t1", text: "edit file.md", status: "completed" },
		});
		expect(node.scrollTop).toBe(0);
	});
});
