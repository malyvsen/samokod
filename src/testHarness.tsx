import { act, render, screen } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import { userEvent } from "@testing-library/user-event";
import { vi } from "vitest";
import { testDefaults, testKey } from "./fixtures";
import type { AppEvent, PlanEntry, SessionKey } from "./types";

export const api = {
	getPrefs: vi.fn(),
	validateRepo: vi.fn(),
	openRepo: vi.fn(),
	refreshBranch: vi.fn(),
	createPlan: vi.fn(),
	executePlan: vi.fn(),
	cancelPlan: vi.fn(),
	selectPlan: vi.fn(),
	sendPrompt: vi.fn(),
	retryLast: vi.fn(),
	cancelTurn: vi.fn(),
	answerPermission: vi.fn(),
	setGlobalConfigOption: vi.fn(),
	setPlanMode: vi.fn(),
	setPlanEvergreen: vi.fn(),
	warmSession: vi.fn(),
	loadHistory: vi.fn(),
	scopingTemplate: vi.fn(),
	planMdText: vi.fn(async () => null as string | null),
	onAppEvent: vi.fn(() => () => {}),
};

export const dialog = {
	open: vi.fn(),
};

export function emitAppEvent(apiMock: typeof api, event: AppEvent): void {
	type Handler = (event: AppEvent) => void;
	const calls = apiMock.onAppEvent.mock.calls as unknown as Handler[][];
	if (calls.length === 0) throw new Error("no app event handler");
	act(() => {
		for (const call of calls) {
			call[0]?.(event);
		}
	});
}

export function stubMatchMedia(): void {
	Object.defineProperty(window, "matchMedia", {
		configurable: true,
		writable: true,
		value: () => ({
			matches: false,
			addEventListener: () => {},
			removeEventListener: () => {},
		}),
	});
}

export async function openChat(
	apiMock: typeof api,
	overrides: { plans?: PlanEntry[]; selected?: SessionKey } = {},
): Promise<UserEvent> {
	if (overrides.plans !== undefined) {
		apiMock.openRepo.mockResolvedValue({
			repo_root: "/repo",
			branch: "feature",
			plans: overrides.plans,
			selected: overrides.selected ?? testKey(overrides.plans[0]?.name ?? "a"),
			config_defaults: testDefaults(),
		});
	}
	const { App } = await import("./App");
	const user = userEvent.setup();
	render(<App />);
	await user.click(await screen.findByRole("button", { name: "open" }));
	await screen.findByRole("textbox", { name: "Ask for a change…" });
	return user;
}
