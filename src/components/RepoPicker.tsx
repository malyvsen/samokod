import type { RecentRepo } from "../types";

export function RepoPicker({
	title,
	subtitle,
	recent,
	error,
	openPaths,
	onOpen,
	onFocusRepo,
	onBrowse,
	onDismissError,
}: {
	title: string;
	subtitle: string;
	recent: RecentRepo[];
	error: string | null;
	openPaths: string[];
	onOpen: (path: string) => void;
	onFocusRepo: (path: string) => void;
	onBrowse: () => void;
	onDismissError: () => void;
}) {
	return (
		<div className="picker">
			<h2>{title}</h2>
			<p className="sub">{subtitle}</p>
			{recent.map((repo) => {
				const openElsewhere = openPaths.includes(repo.path);
				return (
					<div
						className={openElsewhere ? "rrow open-else" : "rrow"}
						key={repo.path}
					>
						<span>{repo.path}</span>
						{openElsewhere && <span className="opentag">OPEN ELSEWHERE</span>}
						{openElsewhere ? (
							<button
								className="tbtn switchbtn"
								type="button"
								onClick={() => onFocusRepo(repo.path)}
							>
								switch
							</button>
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
		</div>
	);
}
