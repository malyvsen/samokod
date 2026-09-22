export function TopBar({
	repoLabel,
	branch,
	working,
	statusText,
	onOpenPicker,
	onNewChat,
}: {
	repoLabel: string;
	branch: string;
	working: boolean;
	statusText: string;
	onOpenPicker: () => void;
	onNewChat: () => void;
}) {
	const tip = working
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
				disabled={working}
				onClick={onNewChat}
			>
				new chat
			</button>
			<span className={`status${working ? " live" : ""}`}>{statusText}</span>
		</div>
	);
}
