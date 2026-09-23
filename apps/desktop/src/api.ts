import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
	AppEvent,
	ConfigOptionView,
	Prefs,
	RepoInfo,
	SessionInfo,
} from "./types";

export function getPrefs(): Promise<Prefs> {
	return invoke<Prefs>("get_prefs");
}

export function validateRepo(path: string): Promise<RepoInfo> {
	return invoke("validate_repo_path", { path });
}

export function openRepo(path: string): Promise<SessionInfo> {
	return invoke<SessionInfo>("open_repo", { path });
}

export function newChat(): Promise<SessionInfo> {
	return invoke<SessionInfo>("new_chat");
}

export function sendPrompt(text: string): Promise<void> {
	return invoke("send_prompt", { text });
}

export function retryLast(): Promise<boolean> {
	return invoke<boolean>("retry_last");
}

export function cancelTurn(): Promise<void> {
	return invoke("cancel_turn");
}

export function answerPermission(
	toolCallId: string,
	optionId: string | null,
): Promise<void> {
	return invoke("answer_permission", { toolCallId, optionId });
}

export function setConfigOption(
	configId: string,
	value: string,
): Promise<ConfigOptionView[]> {
	return invoke("set_config_option", { configId, value });
}

export function onAppEvent(handler: (event: AppEvent) => void): () => void {
	let cancelled = false;
	let unlisten: (() => void) | undefined;
	listen<AppEvent>("samokod://event", (event) => {
		handler(event.payload);
	})
		.then((stop) => {
			if (cancelled) {
				stop();
			} else {
				unlisten = stop;
			}
		})
		.catch((error: unknown) => {
			console.warn("failed to subscribe to app events", error);
		});
	return () => {
		cancelled = true;
		unlisten?.();
	};
}
