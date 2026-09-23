import type { AgentStatus } from "../types";

const STATUS: Record<AgentStatus, { text: string; className: string }> = {
	idle: { text: "IDLE", className: "status" },
	working: { text: "● WORKING", className: "status live" },
	approval: { text: "● PAUSED - APPROVAL", className: "status paused" },
};

export function TopBar({
	repoLabel,
	branch,
	status,
	onOpenPicker,
	onNewChat,
}: {
	repoLabel: string;
	branch: string;
	status: AgentStatus;
	onOpenPicker: () => void;
	onNewChat: () => void;
}) {
	const busy = status !== "idle";
	const { text, className } = STATUS[status];
	const tip = busy
		? "stop the agent to switch repositories"
		: "open repo picker";
	return (
		<div className="topbar">
			<span className="brand">SAMOKOD</span>
			<button
				className="tbtn repo-btn"
				type="button"
				data-tip={tip}
				onClick={onOpenPicker}
			>
				<b>{repoLabel}</b> · {branch}
			</button>
			<button
				className="tbtn"
				type="button"
				disabled={busy}
				onClick={onNewChat}
			>
				new chat
			</button>
			{/* Remount on swap so the pulse animation leaves no stale paint in the WebView compositor */}
			<span key={status} className={className}>
				{text}
			</span>
		</div>
	);
}
