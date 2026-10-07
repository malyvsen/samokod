import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
	AppEvent,
	ConfigOptionView,
	OpenRepoResult,
	PlansUpdate,
	Prefs,
	RepoInfo,
	SessionKey,
} from "./types";

export function getPrefs(): Promise<Prefs> {
	return invoke<Prefs>("get_prefs");
}

export function validateRepo(path: string): Promise<RepoInfo> {
	return invoke("validate_repo_path", { path });
}

export function openRepo(path: string): Promise<OpenRepoResult> {
	return invoke<OpenRepoResult>("open_repo", { path });
}

export function refreshBranch(): Promise<string> {
	return invoke<string>("refresh_branch");
}

export function createPlan(): Promise<PlansUpdate> {
	return invoke<PlansUpdate>("create_plan");
}

export function cancelPlan(session: SessionKey): Promise<PlansUpdate> {
	return invoke<PlansUpdate>("cancel_plan", { session });
}

export function selectPlan(session: SessionKey): Promise<PlansUpdate> {
	return invoke<PlansUpdate>("select_plan", { session });
}

export function setPlanMode(
	plan: string,
	manual: boolean,
): Promise<PlansUpdate> {
	return invoke<PlansUpdate>("set_plan_mode", { plan, manual });
}

export function sendPrompt(session: SessionKey, text: string): Promise<void> {
	return invoke("send_prompt", { session, text });
}

export function scopingTemplate(session: SessionKey): Promise<string | null> {
	return invoke<string | null>("scoping_template", { session });
}

export function retryLast(session: SessionKey): Promise<boolean> {
	return invoke<boolean>("retry_last", { session });
}

export function cancelTurn(session: SessionKey): Promise<void> {
	return invoke("cancel_turn", { session });
}

export function answerPermission(
	session: SessionKey,
	toolCallId: string,
	optionId: string | null,
): Promise<void> {
	return invoke("answer_permission", { session, toolCallId, optionId });
}

export function setConfigOption(
	session: SessionKey,
	configId: string,
	value: string,
): Promise<ConfigOptionView[]> {
	return invoke("set_config_option", { session, configId, value });
}

export function warmSession(session: SessionKey): Promise<void> {
	return invoke("warm_session", { session });
}

export function loadHistory(session: SessionKey): Promise<void> {
	return invoke("load_history", { session });
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
