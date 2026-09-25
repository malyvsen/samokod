import type { RecentRepo } from "../types";

export function RepoPicker({
	title,
	subtitle,
	recent,
	error,
	onOpen,
	onBrowse,
	onDismissError,
}: {
	title: string;
	subtitle: string;
	recent: RecentRepo[];
	error: string | null;
	onOpen: (path: string) => void;
	onBrowse: () => void;
	onDismissError: () => void;
}) {
	return (
		<div className="picker">
			<h2>{title}</h2>
			<p className="sub">{subtitle}</p>
			{recent.map((repo) => (
				<div className="rrow" key={repo.path}>
					<span>{repo.path}</span>
					<button
						className="tbtn"
						type="button"
						onClick={() => onOpen(repo.path)}
					>
						open
					</button>
				</div>
			))}
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
		</div>
	);
}
