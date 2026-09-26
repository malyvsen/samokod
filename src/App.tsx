import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
	abandonPlan,
	answerPermission,
	cancelExecution,
	cancelTurn,
	createPlan,
	executePlan,
	getPrefs,
	markCompleted,
	onAppEvent,
	openRepo,
	refreshBranch,
	retryLast,
	sendPrompt,
	setConfigOption,
	validateRepo,
} from "./api";
import { reducedMotion, useAuroraMotion } from "./auroraMotion";
import { DraftBubble } from "./components/DraftBubble";
import { PlansPanel } from "./components/PlansPanel";
import { RepoPicker } from "./components/RepoPicker";
import { SidePanel } from "./components/SidePanel";
import { TopBar } from "./components/TopBar";
import { Transcript } from "./components/Transcript";
import {
	agentLabelForPhase,
	agentStatusOf,
	isReadOnly,
	selectedChat,
	selectedEntry,
} from "./sessions/select";
import {
	applySessionEvent,
	type ChatState,
	type Chats,
	carryHistory as carryChats,
	errorItem,
	updateEntry,
} from "./sessions/store";
import type {
	AppEvent,
	PlanEntry,
	PlansUpdate,
	RecentRepo,
	SessionKey,
} from "./types";
import { sessionKeyOf } from "./types";
import "./App.css";

type View = { kind: "picker" } | { kind: "chat" };

export function App() {
	const [view, setView] = useState<View>({ kind: "picker" });
	const [repoRoot, setRepoRoot] = useState<string | null>(null);
	const [branch, setBranch] = useState("HEAD");
	const [recent, setRecent] = useState<RecentRepo[]>([]);
	const [pickerError, setPickerError] = useState<string | null>(null);
	const [plans, setPlans] = useState<PlanEntry[]>([]);
	const [selectedKey, setSelectedKey] = useState<SessionKey | null>(null);
	const [chats, setChats] = useState<Chats>({});
	const appRef = useRef<HTMLDivElement>(null);
	const notifyEdit = useAuroraMotion(appRef);
	const transcriptRef = useRef<HTMLDivElement>(null);
	const configGeneration = useRef(0);

	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);
	const chat: ChatState = selectedChat(chats, selectedKey);

	const status = agentStatusOf(chat);
	const busy = chat.working || chat.approval;

	const selectedRef = useRef<SessionKey | null>(null);
	selectedRef.current = selectedKey;

	const updateChat = useCallback(
		(key: SessionKey, next: (chat: ChatState) => ChatState) => {
			setChats((current) => updateEntry(current, key, next));
		},
		[],
	);

	const applyPlans = useCallback((update: PlansUpdate) => {
		setPlans(update.plans);
		setSelectedKey(update.selected);
	}, []);

	const entry = selectedEntry(plans, selectedKey);
	const readOnly = isReadOnly(entry);
	const agentLabel = agentLabelForPhase(entry?.phase);

	useEffect(() => {
		let cancelled = false;
		getPrefs()
			.then((prefs) => {
				if (cancelled) return;
				setRecent(prefs.recent);
			})
			.catch((error: unknown) => {
				console.warn("failed to load prefs", error);
			});
		return () => {
			cancelled = true;
		};
	}, []);

	const handleEvent = useCallback(
		(event: AppEvent) => {
			if (event.type === "plans_changed") {
				applyPlans({ plans: event.plans, selected: event.selected });
				return;
			}
			if (event.type === "branch_changed") {
				setBranch(event.branch);
				return;
			}
			setChats((current) => applySessionEvent(current, event));
		},
		[applyPlans],
	);

	useEffect(() => onAppEvent(handleEvent), [handleEvent]);

	useEffect(() => {
		if (view.kind !== "chat") return;
		function onFocus() {
			refreshBranch()
				.then((fetched) => {
					setBranch(fetched);
				})
				.catch((error: unknown) => {
					console.warn("failed to refresh branch", error);
				});
		}
		window.addEventListener("focus", onFocus);
		return () => {
			window.removeEventListener("focus", onFocus);
		};
	}, [view.kind]);

	const transcript = chat.transcript;
	// biome-ignore lint/correctness/useExhaustiveDependencies: re-scroll whenever the selected transcript identity changes
	useEffect(() => {
		const node = transcriptRef.current;
		if (node !== null) {
			node.scrollTop = node.scrollHeight;
		}
	}, [transcript]);

	useEffect(() => {
		function onPointerMove(event: MouseEvent) {
			if (reducedMotion()) return;
			const node = appRef.current;
			if (node === null) return;
			const rect = node.getBoundingClientRect();
			node.style.setProperty(
				"--mx",
				((event.clientX - rect.left) / rect.width - 0.5).toFixed(3),
			);
			node.style.setProperty(
				"--my",
				((event.clientY - rect.top) / rect.height - 0.5).toFixed(3),
			);
		}
		window.addEventListener("mousemove", onPointerMove);
		return () => {
			window.removeEventListener("mousemove", onPointerMove);
		};
	}, []);

	async function handleOpenPath(path: string) {
		setPickerError(null);
		try {
			const info = await validateRepo(path);
			if (info.root === repoRoot) {
				setView({ kind: "chat" });
				return;
			}
			const opened = await openRepo(info.root);
			setRepoRoot(opened.repo_root);
			setBranch(opened.branch);
			setChats({});
			applyPlans({ plans: opened.plans, selected: opened.selected });
			setView({ kind: "chat" });
			setPickerError(null);
			setRecent((await getPrefs()).recent);
		} catch (error) {
			setPickerError(error instanceof Error ? error.message : String(error));
		}
	}

	async function handleBrowse() {
		setPickerError(null);
		const picked = await open({ directory: true, multiple: false });
		if (picked === null || Array.isArray(picked)) return;
		await handleOpenPath(picked);
	}

	function appendError(key: SessionKey, raw: string) {
		updateChat(key, (chat) => ({
			...chat,
			transcript: [...chat.transcript, errorItem(raw, "retry the turn", true)],
		}));
	}

	async function runTurn(key: SessionKey, text: string) {
		updateChat(key, (chat) => ({ ...chat, working: true, failed: false }));
		try {
			await sendPrompt(key, text);
		} catch (error) {
			updateChat(key, (chat) => ({ ...chat, working: false }));
			appendError(key, error instanceof Error ? error.message : String(error));
		}
	}

	async function handleSend(text: string) {
		const key = selectedRef.current;
		if (text === "" || busy || readOnly || key === null) return;
		updateChat(key, (chat) => ({
			...chat,
			transcript: [
				...chat.transcript,
				{ kind: "user", id: crypto.randomUUID(), text },
			],
		}));
		await runTurn(key, text);
	}

	async function handleStop() {
		const key = selectedRef.current;
		if (key === null) return;
		try {
			await cancelTurn(key);
		} finally {
			updateChat(key, (chat) => ({
				...chat,
				working: false,
				approval: false,
			}));
		}
	}

	function handleSelect(key: SessionKey) {
		setSelectedKey(key);
	}

	async function handleAnswer(toolCallId: string, optionId: string) {
		const key = selectedRef.current;
		if (readOnly || key === null) return;
		await answerPermission(key, toolCallId, optionId);
		updateChat(key, (chat) => ({
			...chat,
			approval: false,
			working: true,
		}));
	}

	async function handleConfigChange(configId: string, value: string) {
		const key = selectedRef.current;
		if (busy || readOnly || key === null) return;
		configGeneration.current += 1;
		const generation = configGeneration.current;
		try {
			const options = await setConfigOption(key, configId, value);
			if (configGeneration.current !== generation) return;
			updateChat(key, (chat) => ({ ...chat, configOptions: options }));
		} catch (error) {
			console.warn(`set_config_option ${configId}=${value} failed`, error);
			return;
		}
	}

	async function handleRetry() {
		const key = selectedRef.current;
		if (busy || readOnly || key === null) return;
		updateChat(key, (chat) => ({ ...chat, failed: false }));
		try {
			const retried = await retryLast(key);
			if (retried) {
				updateChat(key, (chat) => ({ ...chat, working: true }));
			}
		} catch (error) {
			appendError(key, error instanceof Error ? error.message : String(error));
		}
	}

	async function handleNewPlan() {
		try {
			const update = await createPlan();
			applyPlans(update);
		} catch (error) {
			const key = selectedRef.current;
			if (key !== null) {
				appendError(
					key,
					error instanceof Error ? error.message : String(error),
				);
			}
		}
	}

	async function handleExecute(key: SessionKey) {
		updateChat(key, (chat) => ({ ...chat, working: true }));
		try {
			const update = await executePlan(key);
			setChats((current) => carryChats(current, key, update, true));
			applyPlans(update);
		} catch (error) {
			updateChat(key, (chat) => ({ ...chat, working: false }));
			appendError(key, error instanceof Error ? error.message : String(error));
		}
	}

	async function handleDone(key: SessionKey) {
		await runPlansAction(key, (session) => markCompleted(session));
	}

	async function handleAbandon(key: SessionKey) {
		await runPlansAction(key, (session) => abandonPlan(session));
	}

	async function handleCancel(key: SessionKey) {
		await runPlansAction(key, (session) => cancelExecution(session));
	}

	async function runPlansAction(
		key: SessionKey,
		action: (session: SessionKey) => Promise<PlansUpdate>,
	) {
		updateChat(key, (chat) => ({ ...chat, working: true }));
		try {
			const update = await action(key);
			setChats((current) => carryChats(current, key, update, false));
			applyPlans(update);
		} catch (error) {
			updateChat(key, (chat) => ({ ...chat, working: false }));
			appendError(key, error instanceof Error ? error.message : String(error));
		}
	}

	const repoLabel = repoRoot === null ? "no repo" : shortPath(repoRoot);

	return (
		<div className="app" data-state={status} ref={appRef}>
			<div className="aurora a" />
			<div className="rays">
				<i />
				<i />
				<i />
				<i />
				<i />
				<i />
				<i />
				<i />
				<i />
				<i />
			</div>
			<div className="aurora b" />
			<div className="grain" />
			{view.kind === "picker" ? (
				<RepoPicker
					title="SAMOKOD"
					subtitle="open a git repository to start planning"
					recent={recent}
					error={pickerError}
					onOpen={handleOpenPath}
					onBrowse={handleBrowse}
					onDismissError={() => setPickerError(null)}
				/>
			) : (
				<>
					<TopBar
						repoLabel={repoLabel}
						branch={branch}
						status={status}
						onStop={handleStop}
					/>
					<div className="mainrow">
						<PlansPanel
							plans={plans}
							selected={selectedKey}
							onSelect={handleSelect}
							onNewPlan={() => void handleNewPlan()}
							onExecute={(key) => void handleExecute(key)}
							onAbandon={(key) => void handleAbandon(key)}
							onCancel={(key) => void handleCancel(key)}
							onDone={(key) => void handleDone(key)}
						/>
						<div className="chatcol">
							<div className="transcript" ref={transcriptRef}>
								<Transcript
									items={chat.transcript}
									repoLabel={repoLabel}
									agentLabel={agentLabel}
									onRetry={readOnly ? null : handleRetry}
									onAnswer={handleAnswer}
								>
									{!busy && !readOnly && (
										<DraftBubble onSend={handleSend} onEdit={notifyEdit} />
									)}
									{readOnly && (
										<div className="ro-note">
											This session is read-only - the plan is finished.
										</div>
									)}
								</Transcript>
							</div>
						</div>
						<SidePanel
							todos={chat.todos}
							spend={chat.spend}
							sessionId={selectedId ?? ""}
							options={chat.configOptions}
							disabled={busy || readOnly}
							onChange={handleConfigChange}
						/>
					</div>
				</>
			)}
		</div>
	);
}

function shortPath(path: string): string {
	const home = typeof process !== "undefined" ? process.env.HOME : undefined;
	if (home !== undefined && home !== "" && path.startsWith(`${home}/`)) {
		return `~/${path.slice(home.length + 1)}`;
	}
	return path;
}
