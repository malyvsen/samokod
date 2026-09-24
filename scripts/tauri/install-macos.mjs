import { execFileSync } from "node:child_process";
import { accessSync, constants, readdirSync, rmSync } from "node:fs";
import { basename, join } from "node:path";

const [artifactDir, applicationsDir, ...extraArgs] = process.argv.slice(2);
if (
	artifactDir === undefined ||
	artifactDir === "" ||
	applicationsDir === undefined ||
	applicationsDir === "" ||
	extraArgs.length > 0
) {
	throw new Error("usage: install-macos.mjs <artifact-dir> <applications-dir>");
}

const apps = findApps(artifactDir);
if (apps.length !== 1) {
	throw new Error(`expected one .app in ${artifactDir}, found ${apps.length}`);
}

accessSync(applicationsDir, constants.W_OK);
const app = apps[0];
const destination = join(applicationsDir, basename(app));
// Remove first: merging over the old bundle with ditto leaves
// LaunchServices/IconServices serving the stale cached icon.
if (!destination.endsWith(".app")) {
	throw new Error(`refusing to remove non-app destination: ${destination}`);
}
rmSync(destination, { force: true, recursive: true });
execFileSync("ditto", [app, destination], { stdio: "inherit" });
execFileSync("touch", [destination]);

function findApps(directory) {
	const apps = [];
	for (const entry of readdirSync(directory, { withFileTypes: true })) {
		const path = join(directory, entry.name);
		if (entry.isDirectory() && entry.name.endsWith(".app")) {
			apps.push(path);
		} else if (entry.isDirectory()) {
			apps.push(...findApps(path));
		}
	}
	return apps;
}
