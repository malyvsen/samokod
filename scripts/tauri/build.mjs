import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, readdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

const [artifactDirArg, ...extraArgs] = process.argv.slice(2);
if (
	artifactDirArg === undefined ||
	artifactDirArg === "" ||
	extraArgs.length > 0
) {
	throw new Error("usage: build.mjs <artifact-dir>");
}

const workspace = execFileSync("git", ["rev-parse", "--show-toplevel"], {
	encoding: "utf8",
}).trim();
const metadata = JSON.parse(
	execFileSync(
		"cargo",
		[
			"metadata",
			"--format-version",
			"1",
			"--locked",
			"--no-deps",
			"--manifest-path",
			"src-tauri/Cargo.toml",
		],
		{ encoding: "utf8" },
	),
);
const bundle = join(metadata.target_directory, "release", "bundle");
const releaseRoot = join(workspace, "release");
const artifactDir = resolve(artifactDirArg);
if (dirname(artifactDir) !== releaseRoot) {
	throw new Error(
		`artifact directory must name one project under ${releaseRoot}`,
	);
}

rmSync(bundle, { force: true, recursive: true });
const bundles = process.platform === "darwin" ? ["--bundles", "app"] : [];
const buildArgs = ["tauri", "build", ...bundles, "--", "--locked"];
if (process.platform === "win32") {
	execFileSync(
		process.env.ComSpec ?? "cmd.exe",
		["/d", "/s", "/c", `pnpm ${buildArgs.join(" ")}`],
		{ stdio: "inherit" },
	);
} else {
	execFileSync("pnpm", buildArgs, { stdio: "inherit" });
}

const outputs = readdirSync(bundle);
if (outputs.length === 0) {
	throw new Error(`no distributable produced in ${bundle}`);
}

rmSync(artifactDir, { force: true, recursive: true });
mkdirSync(dirname(artifactDir), { recursive: true });
if (process.platform === "darwin") {
	execFileSync("ditto", [bundle, artifactDir]);
} else {
	cpSync(bundle, artifactDir, { recursive: true });
}
