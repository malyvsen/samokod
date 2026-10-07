import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
	answerPermission,
	cancelPlan,
	cancelTurn,
	createPlan,
	executePlan,
	getPrefs,
	loadHistory,
	onAppEvent,
	openRepo,
	refreshBranch,
	retryLast,
	selectPlan,
	sendPrompt,
	setConfigOption,
	setPlanMode,
	validateRepo,
} from "./api";
import { reducedMotion, useAuroraMotion } from "./auroraMotion";
import { DraftBubble } from "./components/DraftBubble";
import { PlansPanel } from "./components/PlansPanel";
import { QueuedBubbleList } from "./components/QueuedBubble";
import { RepoPicker } from "./components/RepoPicker";
import { SidePanel } from "./components/SidePanel";
import { toSelectorModel } from "./components/selectors";
import { TopBar } from "./components/TopBar";
import { Transcript } from "./components/Transcript";
import { hasUserMessage, useSessionDrafts } from "./sessions/drafts";
import { previewPromiseFor, useScopingPreview } from "./sessions/preview";
import { useSessionQueues } from "./sessions/queue";
import { usePinnedTranscript } from "./sessions/scroll";
import {
	agentStatusOf,
	isReadOnly,
	isSessionBusy,
	selectedChat,
	selectedEntry,
} from "./sessions/select";
import { useSessionStart } from "./sessions/start";
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
	RepoDefaults,
	SessionKey,
	TranscriptItem,
} from "./types";
import { sameSession, sessionKeyOf } from "./types";
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
	const [configDefaults, setConfigDefaults] = useState<RepoDefaults>({
		model: null,
		effort: null,
	});
	const appRef = useRef<HTMLDivElement>(null);
	const notifyEdit = useAuroraMotion(appRef);
	const configGeneration = useRef(0);

	const selectedId = selectedKey === null ? null : sessionKeyOf(selectedKey);
	const chat: ChatState | null = selectedChat(chats, selectedKey);
	const draft = useSessionDrafts(selectedKey);
	const queue = useSessionQueues(selectedKey);

	const status = agentStatusOf(chat);
	const busy = isSessionBusy(chat);

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
		setConfigDefaults(update.config_defaults);
	}, []);

	const entry = selectedEntry(plans, selectedKey);
	const readOnly = isReadOnly(entry);
	useSessionStart(selectedKey, chats, readOnly);
	useScopingPreview(selectedKey, updateChat);
	const selectors =
		chat === null
			? { kind: "pending" as const, defaults: configDefaults }
			: readOnly
				? { kind: "live" as const, options: chat.configOptions }
				: toSelectorModel(chat.configOptions, configDefaults);

	const sendNow = useCallback(
		async (key: SessionKey, text: string) => {
			const template = await previewPromiseFor(key);
			updateChat(key, (chat) => ({
				...chat,
				scopingPreview: template ?? chat.scopingPreview,
				transcript: [
					...chat.transcript,
					{
						kind: "user",
						id: crypto.randomUUID(),
						text,
					} satisfies TranscriptItem,
				],
			}));
			updateChat(key, (chat) => ({ ...chat, working: true, failed: false }));
			try {
				await sendPrompt(key, text);
			} catch (error) {
				updateChat(key, (chat) => ({ ...chat, working: false }));
				updateChat(key, (chat) => ({
					...chat,
					transcript: [
						...chat.transcript,
						errorItem(
							error instanceof Error ? error.message : String(error),
							"retry the turn",
							true,
						),
					],
				}));
			}
		},
		[updateChat],
	);

	const maybeDrain = useCallback(
		(key: SessionKey) => {
			const head = queue.dequeueHead(key);
			if (head === null) return;
			void sendNow(key, head.text);
		},
		[queue.dequeueHead, sendNow],
	);

	const handleEvent = useCallback(
		(event: AppEvent) => {
			if (event.type === "plans_changed") {
				setPlans(event.plans);
				setSelectedKey(event.selected);
				return;
			}
			if (event.type === "branch_changed") {
				setBranch(event.branch);
				return;
			}
			setChats((current) => applySessionEvent(current, event));
			if (event.type === "turn_done") {
				const selected = selectedRef.current;
				if (selected === null || !sameSession(selected, event.session)) return;
				maybeDrain(event.session);
			}
		},
		[maybeDrain],
	);

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

	const transcript = chat?.transcript ?? [];
	const { scrollRef, onScroll } = usePinnedTranscript(selectedId, transcript);

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
			applyPlans(opened);
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

	function appendError(
		key: SessionKey,
		raw: string,
		hint: string,
		retryable: boolean,
	) {
		updateChat(key, (chat) => ({
			...chat,
			transcript: [...chat.transcript, errorItem(raw, hint, retryable)],
		}));
	}

	async function handleSend(text: string) {
		const key = selectedRef.current;
		if (text === "" || readOnly || key === null) return;
		draft.onDraftSent();
		if (busy) {
			queue.enqueue(key, text);
			return;
		}
		await sendNow(key, text);
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
		const current = chats[sessionKeyOf(key)] ?? null;
		if (current?.failed) return;
		if (current?.start.kind === "replaying") return;
		maybeDrain(key);
	}

	async function handleSelect(key: SessionKey) {
		const prev = selectedRef.current;
		if (sameSession(prev, key)) return;
		const prevEmpty =
			prev !== null && prev.role === "scoping" && !hasUserMessage(chats, prev);
		try {
			const update = await selectPlan(key);
			if (prev !== null && prevEmpty) {
				const prevId = sessionKeyOf(prev);
				setChats((current) => {
					const next = { ...current };
					delete next[prevId];
					return next;
				});
			}
			applyPlans(update);
		} catch (error) {
			console.warn("select_plan failed", error);
			if (prev !== null) {
				appendError(
					prev,
					error instanceof Error ? error.message : String(error),
					"couldn't switch plan - try again",
					false,
				);
			}
		}
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
			appendError(
				key,
				error instanceof Error ? error.message : String(error),
				"retry the turn",
				true,
			);
		}
	}

	async function handleHistoryRetry() {
		const key = selectedRef.current;
		if (
			key === null ||
			chat?.start.kind === "preparing" ||
			chat?.start.kind === "replaying"
		)
			return;
		try {
			await loadHistory(key);
		} catch (error) {
			console.warn("load_history retry failed", error);
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
					"couldn't create plan - try again",
					false,
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
			appendError(
				key,
				error instanceof Error ? error.message : String(error),
				"couldn't start execution - try again",
				false,
			);
		}
	}

	async function handleCancel(key: SessionKey) {
		const wasEmpty = key.role === "scoping" && !hasUserMessage(chats, key);
		updateChat(key, (chat) => ({ ...chat, working: true }));
		try {
			const update = await cancelPlan(key);
			if (wasEmpty) {
				setChats((current) => {
					const next = { ...current };
					delete next[sessionKeyOf(key)];
					return next;
				});
			} else {
				setChats((current) => carryChats(current, key, update, false));
			}
			applyPlans(update);
		} catch (error) {
			updateChat(key, (chat) => ({ ...chat, working: false }));
			appendError(
				key,
				error instanceof Error ? error.message : String(error),
				"couldn't cancel plan - try again",
				false,
			);
		}
	}

	async function handleSetMode(plan: string, manual: boolean) {
		try {
			const update = await setPlanMode(plan, manual);
			applyPlans(update);
		} catch (error) {
			const key = selectedRef.current;
			if (key !== null) {
				appendError(
					key,
					error instanceof Error ? error.message : String(error),
					"couldn't switch plan mode - try again",
					false,
				);
			}
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
						chat={chat}
						onStop={handleStop}
					/>
					<div className="mainrow">
						<PlansPanel
							plans={plans}
							selected={selectedKey}
							onSelect={handleSelect}
							onNewPlan={() => void handleNewPlan()}
							onExecute={(key) => void handleExecute(key)}
							onCancel={(key) => void handleCancel(key)}
							onSetMode={(plan, manual) => void handleSetMode(plan, manual)}
						/>
						<div className="chatcol">
							<div className="transcript" ref={scrollRef} onScroll={onScroll}>
								<Transcript
									items={transcript}
									lead={chat?.scopingPreview ?? null}
									start={chat?.start ?? null}
									repoLabel={repoLabel}
									onRetry={readOnly ? null : handleRetry}
									onAnswer={handleAnswer}
									onHistoryRetry={
										chat?.start.kind === "failed" && chat.start.error.retryable
											? handleHistoryRetry
											: null
									}
								>
									{selectedId !== null && !readOnly && (
										<>
											<QueuedBubbleList items={queue.items} />
											<DraftBubble
												key={selectedId}
												initialText={draft.initialText}
												placeholder={
													busy ? "Queue a follow-up…" : "Ask for a change…"
												}
												onSend={handleSend}
												onEdit={notifyEdit}
												onInput={draft.onDraftInput}
											/>
										</>
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
							todos={chat?.todos ?? []}
							spend={chat?.spend ?? null}
							sessionId={selectedId ?? ""}
							selectors={selectors}
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
