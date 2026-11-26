import type { ReactNode } from "react";

export type SidebarMode = "plans" | "settings";

export function flipSidebarMode(mode: SidebarMode): SidebarMode {
	return mode === "plans" ? "settings" : "plans";
}

export function Sidebar({
	mode,
	repoLabel,
	repoRoot,
	branch,
	children,
}: {
	mode: SidebarMode;
	repoLabel: string;
	repoRoot: string | null;
	branch: string;
	children: ReactNode;
}) {
	return (
		<div className={mode === "settings" ? "sidebar settings" : "sidebar"}>
			{children}
			<div
				className="sidebar-footer"
				title={`${repoRoot ?? "no repo"} · ${branch}`}
			>
				{repoLabel} · {branch}
			</div>
		</div>
	);
}
