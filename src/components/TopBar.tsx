import { agentStatusOf, isSessionBusy } from "../sessions/select";
import type { ChatState } from "../sessions/store";
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
	chat,
	onStop,
}: {
	repoLabel: string;
	branch: string;
	chat: ChatState | null;
	onStop: () => void;
}) {
	const status = agentStatusOf(chat);
	const busy = isSessionBusy(chat);
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
				{repoLabel} · {branch}
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
