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
	setGlobalConfigOption,
	setPlanEvergreen,
	setPlanMode,
	validateRepo,
} from "./api";
import { reducedMotion, useAuroraMotion } from "./auroraMotion";
import { DraftBubble } from "./components/DraftBubble";
import { PlanMdPane } from "./components/PlanMdPane";
import { PlansPanel } from "./components/PlansPanel";
import { QueuedBubbleList } from "./components/QueuedBubble";
import { RepoPicker } from "./components/RepoPicker";
import { SettingsPanel } from "./components/SettingsPanel";
import { SidePanel } from "./components/SidePanel";
import { TopBar } from "./components/TopBar";
import { Transcript } from "./components/Transcript";
import {
	hasSentUserMessage,
	shouldKeepOnSwitch,
	useSessionDrafts,
} from "./sessions/drafts";
import { usePlanMd } from "./sessions/planMd";
import { previewPromiseFor, useScopingPreview } from "./sessions/preview";
import {
	type QueueMoveDirection,
	resolveDrain,
	useSessionQueues,
} from "./sessions/queue";
import { usePinnedTranscript } from "./sessions/scroll";
import {
	agentStatusOf,
	drainStatusFor,
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
import { useThoughtTimers } from "./sessions/thoughts";
import type {
	AgentStatus,
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
	const [sidebarMode, setSidebarMode] = useState<"plans" | "settings">("plans");
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
	const chatsRef = useRef<Chats>({});
	chatsRef.current = chats;

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
	useThoughtTimers(selectedKey, chat, updateChat);
	const planMd = usePlanMd(selectedKey, chat?.working ?? false);
	const isScoping = selectedKey?.role === "scoping";

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
		(key: SessionKey, status: AgentStatus) => {
			const sessionId = sessionKeyOf(key);
			const head = queue.takeNext(key, status);
			if (head === null) {
				const snapshot = queue.peek(key);
				const decision = resolveDrain(
					snapshot.items,
					snapshot.editingId,
					status,
				);
				const reason =
					decision.kind === "blocked" ? decision.reason : "blocked";
				console.info(`queue drain skip session=${sessionId} reason=${reason}`);
				return;
			}
			console.info(
				`queue drain session=${sessionId} headId=${head.id} textLen=${head.text.length}`,
			);
			void sendNow(key, head.text);
		},
		[queue.takeNext, queue.peek, sendNow],
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
			if (event.type === "turn_done" || event.type === "history_done") {
				const selected = selectedRef.current;
				const selectedId = selected === null ? "null" : sessionKeyOf(selected);
				console.info(
					`event ${event.type} session=${sessionKeyOf(event.session)} selected=${selectedId}`,
				);
			}
			const next = applySessionEvent(chatsRef.current, event);
			setChats(next);
			if (event.type === "turn_done") {
				maybeDrain(event.session, drainStatusFor(next, event.session));
			}
			if (event.type === "history_done") {
				const status = drainStatusFor(next, event.session);
				if (status === "idle") maybeDrain(event.session, status);
				else
					console.info(
						`queue drain skip session=${sessionKeyOf(event.session)} reason=non-idle`,
					);
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

	function handleQueueEdit(id: string) {
		const key = selectedRef.current;
		if (key === null) return;
		queue.setEditing(key, id);
	}

	function handleQueueCommit(id: string, text: string) {
		const key = selectedRef.current;
		if (key === null) return;
		const sessionId = sessionKeyOf(key);
		const currentItems = queue.items;
		const headId = currentItems[0]?.id ?? null;
		const isHead = headId !== null && headId === id;
		const commitStatus = drainStatusFor(chatsRef.current, key);
		console.info(
			`queue commit session=${sessionId} id=${id} isHead=${isHead} textLen=${text.length} status=${commitStatus}`,
		);
		if (text === "" && !isHead) {
			queue.remove(key, id);
			queue.setEditing(key, null);
			return;
		}
		const nextItems =
			text === ""
				? currentItems.filter((item) => item.id !== id)
				: currentItems.map((item) =>
						item.id === id ? { ...item, text } : item,
					);
		const decision = resolveDrain(nextItems, null, commitStatus);
		if (text === "") {
			queue.remove(key, id);
		} else {
			queue.setText(key, id, text);
		}
		queue.setEditing(key, null);
		if (decision.kind === "drain") {
			const head = decision.head;
			queue.remove(key, head.id);
			console.info(
				`queue drain session=${sessionId} headId=${head.id} textLen=${head.text.length}`,
			);
			void sendNow(key, head.text);
		}
	}

	function handleQueueCancel() {
		const key = selectedRef.current;
		if (key === null) return;
		queue.setEditing(key, null);
	}

	function handleQueueMove(id: string, direction: QueueMoveDirection) {
		const key = selectedRef.current;
		if (key === null) return;
		queue.move(key, id, direction);
	}

	const headId = queue.items[0]?.id ?? null;
	const editingBlocked =
		queue.editingId !== null &&
		queue.editingId === headId &&
		status === "idle" &&
		!busy;

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
		const sessionId = sessionKeyOf(key);
		if (busy) {
			const queueLenAfter = queue.peek(key).items.length + 1;
			console.info(
				`queue enqueue session=${sessionId} queueLenAfter=${queueLenAfter} textLen=${text.length}`,
			);
			queue.enqueue(key, text);
			return;
		}
		const idleStatus = drainStatusFor(chatsRef.current, key);
		const decision = resolveDrain(queue.items, queue.editingId, idleStatus);
		if (decision.kind === "drain") {
			const head = decision.head;
			const queueLenAfter = queue.items.length + 1;
			console.info(
				`queue enqueue session=${sessionId} queueLenAfter=${queueLenAfter} textLen=${text.length}`,
			);
			queue.enqueue(key, text);
			queue.remove(key, head.id);
			console.info(
				`queue drain session=${sessionId} headId=${head.id} textLen=${head.text.length}`,
			);
			await sendNow(key, head.text);
			return;
		}
		if (queue.items.length > 0) {
			const queueLenAfter = queue.items.length + 1;
			console.info(
				`queue enqueue session=${sessionId} queueLenAfter=${queueLenAfter} textLen=${text.length}`,
			);
			queue.enqueue(key, text);
			return;
		}
		console.info(`queue sendNow session=${sessionId} textLen=${text.length}`);
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
		const current = selectedChat(chatsRef.current, key);
		if (current?.failed) return;
		if (current?.start.kind === "replaying") return;
		maybeDrain(key, "idle");
	}

	async function handleSelect(key: SessionKey) {
		const prev = selectedRef.current;
		if (sameSession(prev, key)) return;
		const prevDraft = prev === null ? null : (draft.draftFor(prev) ?? null);
		const keep =
			prev === null
				? true
				: shouldKeepOnSwitch(
						chats,
						prevDraft === null
							? draft.drafts
							: { ...draft.drafts, [sessionKeyOf(prev)]: prevDraft },
						prev,
					);
		try {
			const update = await selectPlan(key, prevDraft);
			if (prev !== null && !keep) {
				const prevKey = prev;
				const prevId = sessionKeyOf(prevKey);
				setChats((current) => {
					const next = { ...current };
					delete next[prevId];
					return next;
				});
				draft.clearDraft(prevKey);
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
		configGeneration.current += 1;
		const generation = configGeneration.current;
		try {
			const options = await setGlobalConfigOption(configId, value);
			if (configGeneration.current !== generation) return;
			setChats((current) => {
				const next: Chats = { ...current };
				for (const id of Object.keys(next)) {
					const existing = next[id];
					if (existing !== undefined) {
						next[id] = { ...existing, configOptions: options };
					}
				}
				return next;
			});
		} catch (error) {
			console.warn(
				`set_global_config_option ${configId}=${value} failed`,
				error,
			);
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
		const current = selectedRef.current;
		const currentDraft =
			current === null ? null : (draft.draftFor(current) ?? null);
		const keep =
			current === null
				? true
				: shouldKeepOnSwitch(
						chats,
						currentDraft === null
							? draft.drafts
							: { ...draft.drafts, [sessionKeyOf(current)]: currentDraft },
						current,
					);
		try {
			const update = await createPlan(currentDraft);
			if (current !== null && !sameSession(update.selected, current) && !keep) {
				const vanished = current;
				setChats((chats) => {
					const next = { ...chats };
					delete next[sessionKeyOf(vanished)];
					return next;
				});
				draft.clearDraft(vanished);
			}
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
		const wasEmpty = key.role === "scoping" && !hasSentUserMessage(chats, key);
		updateChat(key, (chat) => ({ ...chat, working: true }));
		try {
			const update = await cancelPlan(key);
			if (wasEmpty) {
				setChats((current) => {
					const next = { ...current };
					delete next[sessionKeyOf(key)];
					return next;
				});
				draft.clearDraft(key);
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

	async function handleSetEvergreen(plan: string, evergreen: boolean) {
		try {
			const update = await setPlanEvergreen(plan, evergreen);
			applyPlans(update);
		} catch (error) {
			const key = selectedRef.current;
			if (key !== null) {
				appendError(
					key,
					error instanceof Error ? error.message : String(error),
					"couldn't switch evergreen intent - try again",
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
						onToggleSettings={() =>
							setSidebarMode((mode) =>
								mode === "plans" ? "settings" : "plans",
							)
						}
					/>
					<div className="mainrow">
						{sidebarMode === "plans" ? (
							<PlansPanel
								plans={plans}
								selected={selectedKey}
								onSelect={handleSelect}
								onNewPlan={() => void handleNewPlan()}
								onExecute={(key) => void handleExecute(key)}
								onCancel={(key) => void handleCancel(key)}
								onSetMode={(plan, manual) => void handleSetMode(plan, manual)}
								onSetEvergreen={(plan, evergreen) =>
									void handleSetEvergreen(plan, evergreen)
								}
							/>
						) : (
							<SettingsPanel
								chats={chats}
								defaults={configDefaults}
								disabled={busy}
								onChange={(configId, value) =>
									void handleConfigChange(configId, value)
								}
							/>
						)}
						<div className="chatcol">
							<div className="transcript" ref={scrollRef} onScroll={onScroll}>
								<Transcript
									items={transcript}
									lead={chat?.scopingPreview ?? null}
									start={chat?.start ?? null}
									repoLabel={repoLabel}
									onRetry={readOnly ? null : handleRetry}
									onAnswer={handleAnswer}
									live={chat?.live ?? null}
									onHistoryRetry={
										chat?.start.kind === "failed" && chat.start.error.retryable
											? handleHistoryRetry
											: null
									}
								>
									{selectedId !== null && !readOnly && (
										<>
											<QueuedBubbleList
												items={queue.items}
												editingId={queue.editingId}
												editingBlocked={editingBlocked}
												onEdit={handleQueueEdit}
												onCommit={handleQueueCommit}
												onCancel={handleQueueCancel}
												onMove={handleQueueMove}
											/>
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
						{isScoping ? (
							<PlanMdPane text={planMd} />
						) : (
							<SidePanel todos={chat?.todos ?? []} />
						)}
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
