import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, test } from "vitest";

const css = readFileSync(join(process.cwd(), "src/App.css"), "utf8");

function ruleBody(selector: string): string {
	const match = css.match(new RegExp(`\\${selector}\\s*\\{([^}]*)\\}`));
	const body = match?.[1];
	if (body === undefined) {
		throw new Error(`missing ${selector} rule in App.css`);
	}
	return body;
}

describe("transcript width contract", () => {
	test(".msg bubbles stay inside the transcript column", () => {
		expect(ruleBody(".msg")).toMatch(/max-width:\s*min\(880px,\s*100%\)/);
	});

	test(".approval bubbles stay inside the transcript column", () => {
		expect(ruleBody(".approval")).toMatch(/max-width:\s*min\(880px,\s*100%\)/);
	});

	test("transcript centering stays overflow-safe", () => {
		expect(ruleBody(".transcript")).not.toMatch(/align-items:\s*center/);
		expect(ruleBody(".tcol")).toMatch(/margin:\s*0\s+auto/);
	});
});
