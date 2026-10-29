import type { RecentRepo } from "../bindings";

export function RepoPicker({
	title,
	subtitle,
	recent,
	error,
	openPaths,
	onOpen,
	onBrowse,
	onDismissError,
}: {
	title: string;
	subtitle: string;
	recent: RecentRepo[];
	error: string | null;
	openPaths: Set<string>;
	onOpen: (path: string) => void;
	onBrowse: () => void;
	onDismissError: () => void;
}) {
	return (
		<div className="picker">
			<h2>{title}</h2>
			<p className="sub">{subtitle}</p>
			{recent.map((repo) => {
				const openElsewhere = openPaths.has(repo.path);
				return (
					<div
						className={openElsewhere ? "rrow open-else" : "rrow"}
						key={repo.path}
					>
						<span className="rpath">{repo.path}</span>
						{openElsewhere && <span className="opentag">OPEN ELSEWHERE</span>}
						{!openElsewhere && (
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
