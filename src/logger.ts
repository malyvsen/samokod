import {
	attachConsole,
	debug,
	error,
	info,
	trace,
	warn,
} from "@tauri-apps/plugin-log";

type ConsoleMethod = "log" | "debug" | "info" | "warn" | "error";

// console.* goes to the Rust logs in addition to devtools.
// Never throws: outside Tauri the plugin calls reject and the original console remains.
export function initLogging(): void {
	attachConsole().catch(() => {});
	// The plugin has no `log` level, so `console.log` maps to `trace`.
	mirror("log", trace);
	mirror("debug", debug);
	mirror("info", info);
	mirror("warn", warn);
	mirror("error", error);
}

function mirror(
	method: ConsoleMethod,
	logger: (message: string) => Promise<void>,
): void {
	const original = console[method].bind(console);
	console[method] = (...args: unknown[]) => {
		original(...args);
		logger(formatArgs(args)).catch(() => {});
	};
}

function formatArgs(args: unknown[]): string {
	return args.map(formatValue).join(" ");
}

function formatValue(value: unknown): string {
	if (typeof value === "string") return value;
	if (value instanceof Error) return value.stack ?? String(value);
	try {
		return JSON.stringify(value) ?? String(value);
	} catch {
		return String(value);
	}
}
