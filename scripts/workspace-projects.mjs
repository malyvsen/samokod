import { execFileSync } from "node:child_process";
import { existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

const [taskName, ...extraArgs] = process.argv.slice(2);
if (taskName === undefined || taskName === "" || extraArgs.length > 0) {
	throw new Error("usage: workspace-projects.mjs <task>");
}

const projects = [];
for (const parent of ["apps", "packages"]) {
	if (!existsSync(parent)) {
		continue;
	}
	for (const entry of readdirSync(parent, { withFileTypes: true })) {
		if (
			entry.isDirectory() &&
			existsSync(join(parent, entry.name, "Taskfile.yml"))
		) {
			projects.push(`${parent}/${entry.name}`);
		}
	}
}

for (const project of projects.sort()) {
	const listed = JSON.parse(
		execFileSync("task", ["-d", project, "--list-all", "--json"], {
			encoding: "utf8",
			stdio: ["ignore", "pipe", "inherit"],
		}),
	);
	if (listed.tasks.some((task) => task.name === taskName)) {
		process.stdout.write(`${project}\n`);
	}
}
