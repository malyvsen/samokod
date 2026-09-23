import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const KINDS = new Set(["patch", "minor", "major"]);
const [kind, ...extraArgs] = process.argv.slice(2);
if (!KINDS.has(kind) || extraArgs.length > 0) {
	throw new Error("usage: bump.mjs <patch|minor|major>");
}

const cargoPath = "src-tauri/Cargo.toml";
const tauriPath = "src-tauri/tauri.conf.json";
if (!existsSync(cargoPath)) {
	throw new Error(`no ${cargoPath} in the repository root`);
}
if (!existsSync(tauriPath)) {
	throw new Error(`no ${tauriPath} in the repository root`);
}

const cargo = readFileSync(cargoPath, "utf8");
const versionMatch = cargo.match(/^version\s*=\s*"(\d+\.\d+\.\d+)"/m);
if (versionMatch === null) {
	throw new Error("no major.minor.patch [package] version in Cargo.toml");
}
const current = versionMatch[1];
const [major, minor, patch] = current.split(".").map(Number);
const next =
	kind === "patch"
		? `${major}.${minor}.${patch + 1}`
		: kind === "minor"
			? `${major}.${minor + 1}.0`
			: `${major + 1}.0.0`;
const tauri = readFileSync(tauriPath, "utf8");
const config = JSON.parse(tauri);
const currentVersionCode = config.bundle?.android?.versionCode;
if (
	currentVersionCode !== undefined &&
	typeof currentVersionCode !== "number"
) {
	throw new Error(`Android versionCode in ${tauriPath} is not a number`);
}

writeFileSync(
	cargoPath,
	replaceOnce(
		cargo,
		/^(version\s*=\s*")[^"]+(")/m,
		`$1${next}$2`,
		"[package] version",
	),
);
execFileSync(
	"cargo",
	["metadata", "--format-version", "1", "--manifest-path", cargoPath],
	{ stdio: "ignore" },
);

process.stdout.write(`${current} → ${next}\n`);
if (currentVersionCode !== undefined) {
	const nextVersionCode = currentVersionCode + 1;
	writeFileSync(
		tauriPath,
		replaceOnce(
			tauri,
			/("versionCode":\s*)\d+/,
			`$1${nextVersionCode}`,
			"Android versionCode",
		),
	);
	process.stdout.write(`versionCode ${nextVersionCode}\n`);
}

function replaceOnce(source, pattern, replacement, label) {
	const next = source.replace(pattern, replacement);
	if (next === source) {
		throw new Error(`failed to update ${label}`);
	}
	return next;
}
