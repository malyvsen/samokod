import { render, screen } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { App } from "./App";

describe("app shell", () => {
	test("renders the idle shell", () => {
		render(<App />);
		expect(screen.getByTestId("shell")).toBeInTheDocument();
	});
});
