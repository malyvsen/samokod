export function TopBar({ onToggleSettings }: { onToggleSettings: () => void }) {
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
		</div>
	);
}
