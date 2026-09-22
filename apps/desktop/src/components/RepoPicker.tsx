import type { RecentRepo } from "../types";

export function RepoPicker({
	title,
	subtitle,
	recent,
	currentPath,
	error,
	onOpen,
	onBrowse,
	onBack,
	onDismissError,
}: {
	title: string;
	subtitle: string;
	recent: RecentRepo[];
	currentPath: string | null;
	error: string | null;
	onOpen: (path: string) => void;
	onBrowse: () => void;
	onBack: (() => void) | null;
	onDismissError: () => void;
}) {
	return (
		<div className="picker">
			<h2>{title}</h2>
			<p className="sub">{subtitle}</p>
			{recent.map((repo) => {
				const isCurrent = currentPath !== null && repo.path === currentPath;
				return (
					<div className={`rrow${isCurrent ? " current" : ""}`} key={repo.path}>
						<span>{repo.path}</span>
						<small>
							{repo.branch} · {isCurrent ? "current chat" : ""}
						</small>
						{isCurrent ? (
							<span className="curtag">current</span>
						) : (
							<button
								className="tbtn"
								type="button"
								onClick={() => onOpen(repo.path)}
							>
								open
							</button>
						)}
					</div>
				);
			})}
			{error !== null && (
				<div className="perr" role="alert">
					✕ {error}{" "}
					<button
						className="tbtn"
						type="button"
						onClick={onDismissError}
						aria-label="dismiss"
					>
						✕
					</button>
				</div>
			)}
			<button className="tbtn primary browse" type="button" onClick={onBrowse}>
				browse…
			</button>
			{onBack !== null && (
				<button className="tbtn back" type="button" onClick={onBack}>
					back to chat
				</button>
			)}
		</div>
	);
}
