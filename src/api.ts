import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type {
	AppEvent,
	ConfigOptionView,
	OpenRepoEntry,
	OpenRepoResult,
	PlansUpdate,
	Prefs,
	RepoInfo,
	SessionKey,
} from "./bindings";
import { commands } from "./bindings";

type CommandResult<T> =
	| { status: "ok"; data: T }
	| { status: "error"; error: string };

async function unwrap<T>(result: Promise<CommandResult<T>>): Promise<T> {
	const settled = await result;
	if (settled.status === "ok") return settled.data;
	throw settled.error;
}

export function getPrefs(): Promise<Prefs> {
	return unwrap(commands.getPrefs());
}

export function validateRepo(path: string): Promise<RepoInfo> {
	return unwrap(commands.validateRepoPath(path));
}

export function openRepo(path: string): Promise<OpenRepoResult> {
	return unwrap(commands.openRepo(path));
}

export function refreshBranch(): Promise<string> {
	return unwrap(commands.refreshBranch());
}

export function createPlan(draft: string | null): Promise<PlansUpdate> {
	return unwrap(commands.createPlan(draft));
}

export function executePlan(session: SessionKey): Promise<PlansUpdate> {
	return unwrap(commands.executePlan(session));
}

export function cancelPlan(session: SessionKey): Promise<PlansUpdate> {
	return unwrap(commands.cancelPlan(session));
}

export function selectPlan(
	session: SessionKey,
	draft: string | null,
): Promise<PlansUpdate> {
	return unwrap(commands.selectPlan(session, draft));
}

export function setPlanMode(
	plan: string,
	manual: boolean,
): Promise<PlansUpdate> {
	return unwrap(commands.setPlanMode(plan, manual));
}

export function setPlanEvergreen(
	plan: string,
	evergreen: boolean,
): Promise<PlansUpdate> {
	return unwrap(commands.setPlanEvergreen(plan, evergreen));
}

export async function sendPrompt(
	session: SessionKey,
	text: string,
): Promise<void> {
	await unwrap(commands.sendPrompt(session, text));
}

export function saveAttachment(
	session: SessionKey,
	dataBase64: string,
): Promise<string> {
	return unwrap(commands.saveAttachment(session, dataBase64));
}

export function planMdText(session: SessionKey): Promise<string | null> {
	return unwrap(commands.planMdText(session));
}

export function retryLast(session: SessionKey): Promise<boolean> {
	return unwrap(commands.retryLast(session));
}

export async function cancelTurn(session: SessionKey): Promise<void> {
	await unwrap(commands.cancelTurn(session));
}

export async function answerPermission(
	session: SessionKey,
	toolCallId: string,
	optionId: string | null,
): Promise<void> {
	await unwrap(commands.answerPermission(session, toolCallId, optionId));
}

export function setGlobalConfigOption(
	configId: string,
	value: string,
): Promise<ConfigOptionView[]> {
	return unwrap(commands.setGlobalConfigOption(configId, value));
}

export async function warmSession(session: SessionKey): Promise<void> {
	await unwrap(commands.warmSession(session));
}

export async function loadHistory(session: SessionKey): Promise<void> {
	await unwrap(commands.loadHistory(session));
}

export function listOpenRepos(): Promise<OpenRepoEntry[]> {
	return commands.listOpenRepos();
}

export function takePendingRepo(): Promise<string | null> {
	return commands.takePendingRepo();
}

export async function focusWindow(label: string): Promise<void> {
	await unwrap(commands.focusWindow(label));
}

export function newWindow(): Promise<string> {
	return unwrap(commands.newWindow());
}

// True for the owned-repos broadcast (`emit` in the backend), which
// reaches every window. Everything else is window-targeted (`emit_to`)
// and arrives only on the owning WebviewWindow.
function isBroadcast(event: AppEvent): boolean {
	return event.type === "open_repos_changed";
}

export function onAppEvent(handler: (event: AppEvent) => void): () => void {
	const { track, cleanup } = trackingSubscriptions();
	function subscribe(
		start: (accept: (event: AppEvent) => boolean) => Promise<() => void>,
		accept: (event: AppEvent) => boolean,
		label: string,
	) {
		start(accept)
			.then(track)
			.catch((error: unknown) => {
				console.warn(`failed to subscribe to ${label} events`, error);
			});
	}
	// Window-targeted events arrive only on the owning WebviewWindow, so one
	// window's transcript and plans list never leak into another window.
	const window = getCurrentWebviewWindow();
	subscribe(
		(accept) =>
			window.listen<AppEvent>("samokod://event", (event) => {
				if (accept(event.payload)) handler(event.payload);
			}),
		(event) => !isBroadcast(event),
		"window",
	);
	// The owned-repos broadcast reaches every window.
	subscribe(
		(accept) =>
			listen<AppEvent>("samokod://event", (event) => {
				if (accept(event.payload)) handler(event.payload);
			}),
		isBroadcast,
		"app",
	);
	return cleanup;
}

export function onToggleSettings(handler: () => void): () => void {
	return onMenuSignal("samokod://toggle-settings", handler);
}

export function onNewPlan(handler: () => void): () => void {
	return onMenuSignal("samokod://new-plan", handler);
}

function onMenuSignal(event: string, handler: () => void): () => void {
	const { track, cleanup } = trackingSubscriptions();
	// The backend emits window-targeted when a window is focused and
	// broadcasts otherwise; both channel subscriptions receive the emit,
	// so coalesce synchronous duplicates into a single toggle.
	let pending = false;
	function fire() {
		if (pending) return;
		pending = true;
		queueMicrotask(() => {
			pending = false;
		});
		handler();
	}
	const window = getCurrentWebviewWindow();
	window
		.listen(event, fire)
		.then(track)
		.catch((error: unknown) => {
			console.warn(`failed to subscribe to ${event} events`, error);
		});
	listen(event, fire)
		.then(track)
		.catch((error: unknown) => {
			console.warn(`failed to subscribe to ${event} events`, error);
		});
	return cleanup;
}

// Shared stop-list for the dual window/app subscriptions above: cleanup
// before either listen resolves still unlistens, and resubscribes (as in
// StrictMode) never leak the earlier stops.
function trackingSubscriptions(): {
	track: (stop: () => void) => void;
	cleanup: () => void;
} {
	let cancelled = false;
	const stops: (() => void)[] = [];
	function track(stop: () => void) {
		if (cancelled) {
			stop();
		} else {
			stops.push(stop);
		}
	}
	function cleanup() {
		cancelled = true;
		for (const stop of stops) stop();
	}
	return { track, cleanup };
}
