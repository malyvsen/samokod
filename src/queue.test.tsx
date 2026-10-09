import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { QueuedBubbleList } from "./components/QueuedBubble";
import {
	testDefaults,
	testEntry,
	testEntryWith,
	testKey,
	testStatus,
} from "./fixtures";
import { clearPreviewCache } from "./sessions/preview";
import { api, emitAppEvent, openChat, stubMatchMedia } from "./testHarness";
import type { SessionKey } from "./types";

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
	clearPreviewCache();
	stubMatchMedia();
	api.getPrefs.mockResolvedValue({ recent: [{ path: "/repo" }] });
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
	api.sendPrompt.mockResolvedValue(undefined);
	api.cancelTurn.mockResolvedValue(undefined);
});

async function startWorking() {
	api.sendPrompt.mockReturnValue(new Promise(() => {}));
	const user = await openChat(api);
	await user.keyboard("do it{Enter}");
	await screen.findByRole("textbox", { name: "Queue a follow-up…" });
	expect(api.sendPrompt).toHaveBeenCalledTimes(1);
	return user;
}

describe("message queue", () => {
	test("composer stays live with queue placeholder while working", async () => {
		await startWorking();
		expect(screen.getByRole("button", { name: "STOP" })).toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Queue a follow-up…" }),
		).toBeInTheDocument();
	});

	test("enter enqueues without calling sendPrompt", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
		expect(screen.getByText("followup").closest(".body")).not.toBeNull();
		expect(document.querySelector(".msg.user.queued .body p")).not.toBeNull();
	});

	test("turn_done sends the head as a YOU bubble", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "followup"),
		);
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
	});

	test("drains FIFO across two turns", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		const queued = screen.getAllByText("QUEUED");
		expect(queued).toHaveLength(2);
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "first"),
		);
		expect(screen.getAllByText("QUEUED")).toHaveLength(1);
		expect(screen.getByText("second")).toBeInTheDocument();
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "second"),
		);
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
	});

	test("turn_failed holds the queue", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		emitAppEvent(api, {
			type: "turn_failed",
			session: testKey(),
			raw: "boom",
			hint: "retry the turn",
			retryable: true,
		});
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
	});

	test("STOP keeps the queue and sends the head next", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByRole("button", { name: "STOP" }));
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "followup"),
		);
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
	});

	test("approval never auto-drains", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		emitAppEvent(api, {
			type: "permission_asked",
			session: testKey(),
			permission: {
				tool_call_id: "t1",
				title: "Run command",
				tool: "bash",
				options: [
					{ id: "allow", kind: "allow" },
					{ id: "reject", kind: "reject" },
				],
				rule_hint: "hint",
			},
		});
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
	});

	test("history replay never auto-drains", async () => {
		const user = await openChat(api);
		emitAppEvent(api, { type: "history_begin", session: testKey() });
		await screen.findByRole("textbox", { name: "Queue a follow-up…" });
		await user.keyboard("followup{Enter}");
		expect(api.sendPrompt).not.toHaveBeenCalled();
		emitAppEvent(api, { type: "history_done", session: testKey() });
		expect(api.sendPrompt).not.toHaveBeenCalled();
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
	});

	test("queue is per-session across plan switches", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const second = testEntryWith("bbb", "scoping", "Second", false, [
			testStatus("scoping"),
		]);
		const plans = [first, second];
		api.selectPlan.mockImplementation(async (session: SessionKey) => ({
			plans,
			selected: session,
			config_defaults: testDefaults(),
		}));
		api.sendPrompt.mockReturnValue(new Promise(() => {}));
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
		});
		await user.keyboard("do it{Enter}");
		await screen.findByRole("textbox", { name: "Queue a follow-up…" });
		await user.keyboard("aaa followup{Enter}");
		expect(screen.getByText("aaa followup")).toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		expect(screen.queryByText("aaa followup")).not.toBeInTheDocument();
		expect(
			screen.getByRole("textbox", { name: "Ask for a change…" }),
		).toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "First, scoping, opens Scoping",
			}),
		);
		expect(screen.getByText("aaa followup")).toBeInTheDocument();
		expect(screen.getByText("QUEUED")).toBeInTheDocument();
	});
});

describe("queued editing", () => {
	test("click enters edit mode", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByText("followup"));
		expect(
			screen.getByRole("textbox", { name: "Edit queued message" }),
		).toBeInTheDocument();
	});

	test("enter commits new text", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByText("followup"));
		const editor = screen.getByRole("textbox", {
			name: "Edit queued message",
		});
		expect(editor).toBeInTheDocument();
		await user.keyboard(" edited{Enter}");
		expect(
			screen.queryByRole("textbox", { name: "Edit queued message" }),
		).not.toBeInTheDocument();
		expect(screen.getByText("followup edited")).toBeInTheDocument();
		expect(screen.getByText("followup edited").closest(".body")).not.toBeNull();
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
	});

	test("escape reverts", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByText("followup"));
		await user.keyboard(" edited{Escape}");
		expect(
			screen.queryByRole("textbox", { name: "Edit queued message" }),
		).not.toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
		expect(screen.queryByText("followup edited")).not.toBeInTheDocument();
	});

	test("blur reverts", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByText("followup"));
		await user.keyboard(" edited");
		await user.click(
			screen.getByRole("textbox", { name: "Queue a follow-up…" }),
		);
		expect(
			screen.queryByRole("textbox", { name: "Edit queued message" }),
		).not.toBeInTheDocument();
		expect(screen.getByText("followup")).toBeInTheDocument();
	});

	test("enter on emptied text removes the item", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByText("followup"));
		const editor = screen.getByRole("textbox", {
			name: "Edit queued message",
		});
		editor.textContent = "";
		await user.keyboard("{Enter}");
		expect(screen.queryByText("QUEUED")).not.toBeInTheDocument();
		expect(screen.queryByText("followup")).not.toBeInTheDocument();
	});

	test("editing the head holds the drain with yellow outline", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		await user.click(screen.getByText("first"));
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(document.querySelector(".queued.blocked")).not.toBeNull(),
		);
		expect(api.sendPrompt).toHaveBeenCalledTimes(1);
		expect(screen.getAllByText("QUEUED")).toHaveLength(2);
	});

	test("editing a non-head item still drains", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		await user.click(screen.getByText("second"));
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "first"),
		);
		expect(screen.getAllByText("QUEUED")).toHaveLength(1);
	});

	test("editing while working shows no yellow outline", async () => {
		const user = await startWorking();
		await user.keyboard("followup{Enter}");
		await user.click(screen.getByText("followup"));
		expect(document.querySelector(".queued.blocked")).toBeNull();
		expect(
			screen.getByRole("textbox", { name: "Edit queued message" }),
		).toBeInTheDocument();
	});
});

describe("queued reorder", () => {
	function queuedOrder(): string[] {
		return Array.from(document.querySelectorAll(".msg.user.queued")).map(
			(node) => node.textContent ?? "",
		);
	}

	function moveButton(
		name: "Move queued message up" | "Move queued message down",
		index: number,
	): HTMLElement {
		const buttons = screen.getAllByRole("button", { name });
		const button = buttons[index];
		if (button === undefined)
			throw new Error(`missing ${name} at index ${index}`);
		return button;
	}

	function queuedAt(index: number): string {
		const order = queuedOrder();
		const text = order[index];
		if (text === undefined) throw new Error(`missing queued bubble ${index}`);
		return text;
	}

	test("two items show arrows with disabled ends; single hides them", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		expect(
			screen.queryByRole("button", { name: "Move queued message up" }),
		).not.toBeInTheDocument();
		expect(
			screen.queryByRole("button", { name: "Move queued message down" }),
		).not.toBeInTheDocument();
		await user.keyboard("second{Enter}");
		const ups = screen.getAllByRole("button", {
			name: "Move queued message up",
		});
		const downs = screen.getAllByRole("button", {
			name: "Move queued message down",
		});
		expect(ups).toHaveLength(2);
		expect(downs).toHaveLength(2);
		expect(moveButton("Move queued message up", 0)).toBeDisabled();
		expect(moveButton("Move queued message down", 0)).not.toBeDisabled();
		expect(moveButton("Move queued message up", 1)).not.toBeDisabled();
		expect(moveButton("Move queued message down", 1)).toBeDisabled();
	});

	test("clicking down on the first item swaps order and drains the new head", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		expect(queuedAt(0)).toContain("first");
		expect(queuedAt(1)).toContain("second");
		await user.click(moveButton("Move queued message down", 0));
		expect(queuedAt(0)).toContain("second");
		expect(queuedAt(1)).toContain("first");
		emitAppEvent(api, { type: "turn_done", session: testKey() });
		await vi.waitFor(() =>
			expect(api.sendPrompt).toHaveBeenCalledWith(testKey(), "second"),
		);
	});

	test("arrow click and Enter reorder without entering edit", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		await user.click(moveButton("Move queued message down", 0));
		expect(
			screen.queryByRole("textbox", { name: "Edit queued message" }),
		).not.toBeInTheDocument();
		expect(queuedAt(0)).toContain("second");
		const up = moveButton("Move queued message up", 1);
		up.focus();
		await user.keyboard("{Enter}");
		expect(
			screen.queryByRole("textbox", { name: "Edit queued message" }),
		).not.toBeInTheDocument();
		expect(queuedAt(0)).toContain("first");
		expect(queuedAt(1)).toContain("second");
	});

	test("editing keeps arrows; mousedown and click reorder with draft intact", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		await user.click(screen.getByText("first"));
		const editor = screen.getByRole("textbox", {
			name: "Edit queued message",
		});
		expect(editor).toBeInTheDocument();
		expect(
			screen.getAllByRole("button", { name: "Move queued message up" }),
		).toHaveLength(2);
		await user.keyboard(" edited");
		const down = moveButton("Move queued message down", 0);
		fireEvent.mouseDown(down);
		await user.click(down);
		const kept = screen.getByRole("textbox", {
			name: "Edit queued message",
		});
		expect(kept).toBeInTheDocument();
		expect(kept.textContent).toContain("first edited");
		const bubbles = document.querySelectorAll(".msg.user.queued");
		const moved = bubbles[1];
		if (moved === undefined) throw new Error("missing moved bubble");
		expect(moved.textContent).toContain("first edited");
	});

	test("tab to an arrow does not cancel the edit", async () => {
		const user = await startWorking();
		await user.keyboard("first{Enter}");
		await user.keyboard("second{Enter}");
		await user.click(screen.getByText("first"));
		const editor = screen.getByRole("textbox", {
			name: "Edit queued message",
		});
		expect(editor).toBeInTheDocument();
		await user.tab({ shift: true });
		expect(
			screen.getByRole("textbox", { name: "Edit queued message" }),
		).toBeInTheDocument();
		expect(document.activeElement?.getAttribute("aria-label")).toContain(
			"Move queued message",
		);
	});
});

describe("queued collapse", () => {
	const longText = `${"queued long line\n".repeat(11)}final`;

	function collapsedQueuedOrder(): string[] {
		return Array.from(document.querySelectorAll(".msg.user.queued")).map(
			(node) => node.textContent ?? "",
		);
	}

	test("long queued shows move arrows plus plus-minus together", () => {
		const { container } = render(
			<QueuedBubbleList
				items={[
					{ id: "q1", text: longText },
					{ id: "q2", text: longText },
				]}
				editingId={null}
				editingBlocked={false}
				onEdit={vi.fn()}
				onCommit={vi.fn()}
				onCancel={vi.fn()}
				onMove={vi.fn()}
			/>,
		);
		const bubbles = container.querySelectorAll(".msg.user.queued");
		expect(bubbles).toHaveLength(2);
		expect(bubbles[0]?.classList.contains("collapsed")).toBe(true);
		expect(
			screen.getAllByRole("button", { name: "Move queued message up" }),
		).toHaveLength(2);
		expect(
			screen.getAllByRole("button", { name: "Move queued message down" }),
		).toHaveLength(2);
		const toggles = screen.getAllByRole("button", {
			name: "Expand message",
		});
		expect(toggles).toHaveLength(2);
		expect(toggles[0]?.textContent).toBe("+");
		const header = bubbles[0]?.querySelector(".who .right");
		expect(header?.textContent).toContain("↑");
		expect(header?.textContent).toContain("+");
		expect(collapsedQueuedOrder()[0]).toContain("queued long line");
	});

	test("long queued expands on toggle without entering edit", () => {
		const onEdit = vi.fn();
		const { container } = render(
			<QueuedBubbleList
				items={[{ id: "q1", text: longText }]}
				editingId={null}
				editingBlocked={false}
				onEdit={onEdit}
				onCommit={vi.fn()}
				onCancel={vi.fn()}
				onMove={vi.fn()}
			/>,
		);
		const bubble = container.querySelector(".msg.user.queued");
		expect(bubble?.classList.contains("collapsed")).toBe(true);
		fireEvent.click(screen.getByRole("button", { name: "Expand message" }));
		expect(bubble?.classList.contains("collapsed")).toBe(false);
		expect(onEdit).not.toHaveBeenCalled();
		expect(
			screen.getByRole("button", { name: "Collapse message" }).textContent,
		).toBe("-");
	});
});
