import { openUrl } from "@tauri-apps/plugin-opener";
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, test, vi } from "vitest";
import { MessageMarkdown } from "./components/MessageMarkdown";

vi.mock("@tauri-apps/plugin-opener", () => ({
	openUrl: vi.fn(),
}));

const openUrlMock = vi.mocked(openUrl);

beforeEach(() => {
	openUrlMock.mockReset();
	openUrlMock.mockResolvedValue(undefined);
	vi.restoreAllMocks();
});

describe("message markdown links", () => {
	test("https link opens externally and stays in-app", () => {
		render(<MessageMarkdown text="[docs](https://example.com)" />);
		const link = screen.getByRole("link", { name: "docs" });
		expect(link).toHaveAttribute("target", "_blank");
		expect(link).toHaveAttribute("rel", "noopener noreferrer");
		const before = window.location.href;
		fireEvent.click(link);
		expect(openUrlMock).toHaveBeenCalledTimes(1);
		expect(openUrlMock).toHaveBeenCalledWith("https://example.com");
		expect(window.location.href).toBe(before);
	});

	test("relative link never calls opener", () => {
		render(<MessageMarkdown text="[local](docs/file.md)" />);
		const link = screen.getByRole("link", { name: "local" });
		const before = window.location.href;
		fireEvent.click(link);
		expect(openUrlMock).not.toHaveBeenCalled();
		expect(window.location.href).toBe(before);
	});

	test("opener failure logs via console.error", async () => {
		const failure = new Error("boom");
		openUrlMock.mockRejectedValueOnce(failure);
		const errorSpy = vi.spyOn(console, "error").mockImplementation(() => {});
		render(<MessageMarkdown text="[docs](https://example.com)" />);
		fireEvent.click(screen.getByRole("link", { name: "docs" }));
		await vi.waitFor(() => expect(errorSpy).toHaveBeenCalledTimes(1));
		expect(errorSpy.mock.calls[0]?.[0]).toContain("https://example.com");
		expect(errorSpy.mock.calls[0]?.[1]).toBe(failure);
	});
});
