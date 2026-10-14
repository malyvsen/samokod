import { screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import type { OpenRepoResult, PlansUpdate } from "./bindings";
import { testDefaults, testEntryWith, testStatus } from "./fixtures";
import { api, openChat, stubMatchMedia } from "./testHarness";

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
	api.getPrefs.mockResolvedValue({ recent: [{ path: "/repo" }] });
	api.validateRepo.mockResolvedValue({ root: "/repo", branch: "feature" });
	api.warmSession.mockResolvedValue(undefined);
	api.loadHistory.mockResolvedValue(undefined);
	api.sendPrompt.mockResolvedValue(undefined);
});

describe("scoping lead through the wire shape", () => {
	test("open payload renders its lead", async () => {
		const payload: OpenRepoResult = {
			repo_root: "/repo",
			branch: "feature",
			plans: [
				testEntryWith("aaa", "scoping", "First", false, [
					testStatus("scoping"),
				]),
			],
			selected: { plan: "aaa", role: "scoping" },
			config_defaults: testDefaults(),
			scoping_preview: "OPEN-TEMPLATE",
		};
		api.openRepo.mockResolvedValue(payload);
		await openChat(api);
		expect(await screen.findByText("OPEN-TEMPLATE")).toBeInTheDocument();
	});

	test("create payload renders its lead", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const fresh = testEntryWith("ccc", "scoping", "Untitled", false, [
			testStatus("scoping"),
		]);
		const created: PlansUpdate = {
			plans: [first, fresh],
			selected: { plan: "ccc", role: "scoping" },
			config_defaults: testDefaults(),
			scoping_preview: "CREATE-TEMPLATE",
		};
		api.createPlan.mockResolvedValue(created);
		const user = await openChat(api, {
			plans: [first],
			selected: { plan: "aaa", role: "scoping" },
			scoping_preview: null,
		});
		expect(screen.queryByText("CREATE-TEMPLATE")).not.toBeInTheDocument();
		await user.click(screen.getByRole("button", { name: "New plan" }));
		expect(await screen.findByText("CREATE-TEMPLATE")).toBeInTheDocument();
	});

	test("select payload renders its lead", async () => {
		const first = testEntryWith("aaa", "scoping", "First", false, [
			testStatus("scoping"),
		]);
		const second = testEntryWith("bbb", "scoping", "Second", false, [
			testStatus("scoping"),
		]);
		const plans = [first, second];
		api.selectPlan.mockImplementation((session) => {
			const selected: PlansUpdate = {
				plans,
				selected: session,
				config_defaults: testDefaults(),
				scoping_preview: "SELECT-TEMPLATE",
			};
			return Promise.resolve(selected);
		});
		const user = await openChat(api, {
			plans,
			selected: { plan: "aaa", role: "scoping" },
			scoping_preview: null,
		});
		expect(screen.queryByText("SELECT-TEMPLATE")).not.toBeInTheDocument();
		await user.click(
			await screen.findByRole("button", {
				name: "Second, scoping, opens Scoping",
			}),
		);
		expect(await screen.findByText("SELECT-TEMPLATE")).toBeInTheDocument();
	});
});
