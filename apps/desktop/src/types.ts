export interface ConfigValueView {
	value: string;
	name: string;
}

export interface ConfigOptionView {
	id: string;
	name: string;
	current: string;
	options: ConfigValueView[];
}

export interface SessionInfo {
	session_id: string;
	repo_root: string;
	branch: string;
	config_options: ConfigOptionView[];
}

export interface RecentRepo {
	path: string;
	branch: string;
}

export interface RepoInfo {
	root: string;
	branch: string;
}

export interface Prefs {
	recent: RecentRepo[];
	last_repo: string | null;
}
