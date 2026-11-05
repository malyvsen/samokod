export function TopBar({
	repoLabel,
	branch,
	onToggleSettings,
}: {
	repoLabel: string;
	branch: string;
	onToggleSettings: () => void;
}) {
	return (
		<div className="topbar">
			<button
				className="brand"
				type="button"
				onClick={onToggleSettings}
				aria-label="Toggle settings"
			>
				SAMOKOD
			</button>
			<span className="repo-static">
				{repoLabel} · {branch}
			</span>
		</div>
	);
}
