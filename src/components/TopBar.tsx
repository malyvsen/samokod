import type { AgentStatus } from "../types";

const STATUS: Record<AgentStatus, { text: string; className: string }> = {
	idle: { text: "IDLE", className: "status" },
	working: { text: "● WORKING", className: "status live" },
	approval: { text: "● PAUSED - APPROVAL", className: "status paused" },
	failed: { text: "● FAILED", className: "status failed" },
};

export function TopBar({
	repoLabel,
	branch,
	status,
	onStop,
}: {
	repoLabel: string;
	branch: string;
	status: AgentStatus;
	onStop: () => void;
}) {
	const busy = status === "working" || status === "approval";
	const { text, className } = STATUS[status];
	// Remount on swap so the pulse animation leaves no stale paint in the WebView compositor.
	const pill = (
		<span key={status} className={className}>
			{text}
		</span>
	);
	return (
		<div className="topbar">
			<span className="brand">SAMOKOD</span>
			<span className="repo-static">
				<b>{repoLabel}</b> · {branch}
			</span>
			{busy ? (
				<span className="stop-wrap">
					{pill}
					<button className="stop-ctl" type="button" onClick={onStop}>
						<span className="sq" aria-hidden="true" />
						STOP
					</button>
				</span>
			) : (
				pill
			)}
		</div>
	);
}
