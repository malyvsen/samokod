import { beforeEach, describe, expect, test, vi } from "vitest";
import { testKey } from "../fixtures";
import { clearPreviewCache, previewPromiseFor } from "./preview";

const api = vi.hoisted(() => ({
	scopingTemplate: vi.fn(),
}));
vi.mock("../api", () => api);

beforeEach(() => {
	vi.clearAllMocks();
	clearPreviewCache();
	api.scopingTemplate.mockResolvedValue("TEMPLATE");
});

describe("preview promise cache", () => {
	test("concurrent callers share one in-flight fetch", async () => {
		const first = previewPromiseFor(testKey());
		const second = previewPromiseFor(testKey());
		expect(first).toBe(second);
		await expect(first).resolves.toBe("TEMPLATE");
		await expect(second).resolves.toBe("TEMPLATE");
		expect(api.scopingTemplate).toHaveBeenCalledTimes(1);
		expect(api.scopingTemplate).toHaveBeenCalledWith(testKey());
	});

	test("each session fetches at most once", async () => {
		await expect(previewPromiseFor(testKey("aaa"))).resolves.toBe("TEMPLATE");
		await expect(previewPromiseFor(testKey("aaa"))).resolves.toBe("TEMPLATE");
		await expect(previewPromiseFor(testKey("bbb"))).resolves.toBe("TEMPLATE");
		expect(api.scopingTemplate).toHaveBeenCalledTimes(2);
	});

	test("non-scoping roles resolve null without a backend call", async () => {
		await expect(
			previewPromiseFor({ plan: "aaa", role: "executing" }),
		).resolves.toBeNull();
		await expect(
			previewPromiseFor({ plan: "aaa", role: "landing" }),
		).resolves.toBeNull();
		expect(api.scopingTemplate).not.toHaveBeenCalled();
	});

	test("fetch failure resolves null and stays cached", async () => {
		api.scopingTemplate.mockRejectedValue(new Error("boom"));
		await expect(previewPromiseFor(testKey())).resolves.toBeNull();
		await expect(previewPromiseFor(testKey())).resolves.toBeNull();
		expect(api.scopingTemplate).toHaveBeenCalledTimes(1);
	});

	test("backend none resolves null", async () => {
		api.scopingTemplate.mockResolvedValue(null);
		await expect(previewPromiseFor(testKey())).resolves.toBeNull();
		expect(api.scopingTemplate).toHaveBeenCalledTimes(1);
	});

	test("clearing the cache refetches", async () => {
		await expect(previewPromiseFor(testKey())).resolves.toBe("TEMPLATE");
		clearPreviewCache();
		await expect(previewPromiseFor(testKey())).resolves.toBe("TEMPLATE");
		expect(api.scopingTemplate).toHaveBeenCalledTimes(2);
	});
});
