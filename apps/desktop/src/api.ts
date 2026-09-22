import { invoke } from "@tauri-apps/api/core";
import type { Prefs, RepoInfo, SessionInfo } from "./types";

export function getPrefs(): Promise<Prefs> {
	return invoke<Prefs>("get_prefs");
}

export function validateRepo(path: string): Promise<RepoInfo> {
	return invoke("validate_repo_path", { path });
}

export function openRepo(path: string): Promise<SessionInfo> {
	return invoke<SessionInfo>("open_repo", { path });
}
