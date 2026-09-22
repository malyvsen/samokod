import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AppEvent, Prefs, RepoInfo, SessionInfo } from "./types";

export function getPrefs(): Promise<Prefs> {
	return invoke<Prefs>("get_prefs");
}

export function validateRepo(path: string): Promise<RepoInfo> {
	return invoke("validate_repo_path", { path });
}

export function openRepo(path: string): Promise<SessionInfo> {
	return invoke<SessionInfo>("open_repo", { path });
}

export function sendPrompt(text: string): Promise<void> {
	return invoke("send_prompt", { text });
}

export function cancelTurn(): Promise<void> {
	return invoke("cancel_turn");
}

export function onAppEvent(
	handler: (event: AppEvent) => void,
): Promise<() => void> {
	return listen<AppEvent>("samokod://event", (event) => {
		handler(event.payload);
	});
}
