import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
	answerPermission,
	cancelPlan,
	cancelTurn,
	createPlan,
	executePlan,
	getPrefs,
	listOpenRepos,
	loadHistory,
	onAppEvent,
	onNewPlan,
	onToggleSettings,
	openRepo,
	refreshBranch,
	retryLast,
	selectPlan,
	sendPrompt,
	setGlobalConfigOption,
	setPlanEvergreen,
	setPlanMode,
	takePendingRepo,
	validateRepo,
} from "./api";
import { reducedMotion, useAuroraMotion } from "./auroraMotion";
import type {
	AppEvent,
	OpenRepoEntry,
	PlanEntry,
	PlansUpdate,
	RecentRepo,
	RepoDefaults,
	SessionKey,
} from "./bindings";
import { DraftBubble } from "./components/DraftBubble";
import { PlanMdPane } from "./components/PlanMdPane";
import { PlansPanel } from "./components/PlansPanel";
import { QueuedBubbleList } from "./components/QueuedBubble";
import { RepoPicker } from "./components/RepoPicker";
import { SettingsPanel } from "./components/SettingsPanel";
import {
	flipSidebarMode,
	Sidebar,
	type SidebarMode,
} from "./components/Sidebar";
import { CommitsPanel, SidePanel } from "./components/SidePanel";
import { Transcript } from "./components/Transcript";
import {
	hasSentUserMessage,
	shouldKeepOnSwitch,
	useSessionDrafts,
} from "./sessions/drafts";
import { usePlanMd } from "./sessions/planMd";
import {
	type QueueMoveDirection,
	resolveDrain,
	useSessionQueues,
} from "./sessions/queue";
import { usePinnedTranscript } from "./sessions/scroll";
import {
	agentStatusOf,
	draftContextFor,
	drainStatusFor,
	followRename,
	isReadOnly,
	isSessionBusy,
	repairSelection,
	selectedChat,
	selectedEntry,
	sideContentFor,
} from "./sessions/select";
import { useSessionStart } from "./sessions/start";
import {
	applySessionEvent,
	type ChatState,
	type Chats,
	carryHistory,
	errorItem,
	liveTurnOf,
	markAcked,
	markIdle,
	markSent,
	removeEntry,
	updateEntry,
} from "./sessions/store";
import type { AgentStatus, TranscriptItem } from "./types";
import { isWorkRole, sameSession, sessionKeyOf } from "./types";
import "./App.css";

type View = { kind: "picker" } | { kind: "chat" };

export function App() {
	const [view, setView] = useState<View>({ kind: "picker" });
	const [repoRoot, setRepoRoot] = useState<string | null>(null);
	const [branch, setBranch] = useState("HEAD");
	const [recent, setRecent] = useState<RecentRepo[]>([]);
	const [pickerError, setPickerError] = useState<string | null>(null);
	const [openRepos, setOpenRepos] = useState<OpenRepoEntry[]>([]);
	const [plans, setPlans] = useState<PlanEntry[]>([]);
	const [selectedKey, setSelectedKey] = useState<SessionKey | null>(null);
	const [chats, setChats] = useState<Chats>({});
	const [configDefaults, setConfigDefaults] = useState<RepoDefaults>({
		model: null,
		effort: null,
	});
	const [sidebarMode, setSidebarMode] = useState<SidebarMode>("plans");
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
	const handleNewPlanRef = useRef<() => void>(() => {});

	// Single write path for chats. The ref publishes synchronously so events
	// arriving before the next render chain off each other instead of racing.
	const applyChats = useCallback((update: (current: Chats) => Chats): Chats => {
		const next = update(chatsRef.current);
		chatsRef.current = next;
		setChats(next);
		return next;
	}, []);

	const updateChat = useCallback(
		(key: SessionKey, next: (chat: ChatState) => ChatState) => {
			applyChats((current) => updateEntry(current, key, next));
		},
		[applyChats],
	);

	const applyPlans = useCallback((update: PlansUpdate) => {
		setPlans(update.plans);
		setSelectedKey(update.selected);
		setConfigDefaults(update.config_defaults);
	}, []);

	const applyOwnedUpdate = useCallback(
		(oldPlan: string, update: PlansUpdate) => {
			setPlans(update.plans);
			setConfigDefaults(update.config_defaults);
			if (selectedRef.current?.plan === oldPlan) {
				setSelectedKey(followRename(update.plans, oldPlan, update.selected));
			}
		},
		[],
	);

	const entry = selectedEntry(plans, selectedKey);
	const readOnly = isReadOnly(entry);
	useSessionStart(selectedKey, chats, readOnly);
	const planMd = usePlanMd(selectedKey, chat?.working ?? false);
	const isScoping = selectedKey?.role === "scoping";
	const side = sideContentFor(entry, selectedKey, chat?.todos ?? []);
	const draftContextPct = draftContextFor(entry, selectedKey);

	const sendNow = useCallback(
		async (key: SessionKey, text: string) => {
			updateChat(key, (chat) => ({
				...chat,
				transcript: [
					...chat.transcript,
					{
						kind: "user",
						id: crypto.randomUUID(),
						text,
					} satisfies TranscriptItem,
				],
			}));
			updateChat(key, (chat) => markSent(chat));
			try {
				await sendPrompt(key, text);
				updateChat(key, (chat) => markAcked(chat, Date.now()));
			} catch (error) {
				updateChat(key, (chat) => markIdle(chat));
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
				setSelectedKey((prev) => repairSelection(event.plans, prev));
				return;
			}
			if (event.type === "branch_changed") {
				setBranch(event.branch);
				return;
			}
			if (event.type === "open_repos_changed") {
				setOpenRepos(event.repos);
				return;
			}
			if (event.type === "turn_done" || event.type === "history_done") {
				const selected = selectedRef.current;
				const selectedId = selected === null ? "null" : sessionKeyOf(selected);
				console.info(
					`event ${event.type} session=${sessionKeyOf(event.session)} selected=${selectedId}`,
				);
			}
			const next = applyChats((current) => applySessionEvent(current, event));
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
		[maybeDrain, applyChats],
	);

	useEffect(() => {
		let cancelled = false;
		getPrefs()
			.then((prefs) => {
				if (cancelled) return;
				setRecent(prefs.recent ?? []);
			})
			.catch((error: unknown) => {
				console.warn("failed to load prefs", error);
			});
		return () => {
			cancelled = true;
		};
	}, []);

	useEffect(() => onAppEvent(handleEvent), [handleEvent]);

	useEffect(() => onToggleSettings(() => setSidebarMode(flipSidebarMode)), []);

	useEffect(() => onNewPlan(() => handleNewPlanRef.current()), []);

	// Picker marks come from the initial list plus the global
	// `open_repos_changed` handler above.
	useEffect(() => {
		if (view.kind !== "picker") return;
		let cancelled = false;
		listOpenRepos()
			.then((repos) => {
				if (cancelled) return;
				setOpenRepos(repos);
			})
			.catch((error: unknown) => {
				console.warn("failed to list open repos", error);
			});
		return () => {
			cancelled = true;
		};
	}, [view.kind]);

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

	const handleOpenPath = useCallback(
		async (path: string) => {
			setPickerError(null);
			let attempted = path;
			try {
				const info = await validateRepo(path);
				attempted = info.root;
				const opened = await openRepo(info.root);
				setRepoRoot(opened.repo_root);
				setBranch(opened.branch);
				applyChats(() => ({}));
				applyPlans(opened);
				setView({ kind: "chat" });
				setPickerError(null);
				setRecent((await getPrefs()).recent ?? []);
			} catch (error) {
				if (isAlreadyOpenError(error)) {
					setPickerError(
						`${attempted} is already open - switched to that window`,
					);
					return;
				}
				setPickerError(error instanceof Error ? error.message : String(error));
			}
		},
		[applyChats, applyPlans],
	);
	async function handleBrowse() {
		setPickerError(null);
		const picked = await open({ directory: true, multiple: false });
		if (picked === null || Array.isArray(picked)) return;
		await handleOpenPath(picked);
	}

	// CLI launch: the backend staged `samokod <path>` for this window before
	// it was built. Claimed once across StrictMode remounts; the pop is
	// destructive, so the result is never dropped on cleanup.
	const pendingClaimed = useRef(false);
	useEffect(() => {
		if (pendingClaimed.current) return;
		pendingClaimed.current = true;
		takePendingRepo()
			.then((pending) => {
				if (pending === null) return;
				void handleOpenPath(pending);
			})
			.catch((error: unknown) => {
				console.warn("failed to take pending repo", error);
			});
	}, [handleOpenPath]);

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

	function handleAttachmentError(message: string) {
		const key = selectedRef.current;
		if (key === null) return;
		appendError(key, message, "image not saved - try again", false);
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
		const holds = isWorkRole(key.role);
		let stopped = true;
		try {
			await cancelTurn(key);
		} catch (error) {
			stopped = false;
			console.warn("cancel_turn failed", error);
		}
		updateChat(key, (chat) => ({
			...markIdle(chat),
			approval: false,
			transcript:
				stopped && holds
					? [
							...chat.transcript,
							{
								kind: "agent",
								id: crypto.randomUUID(),
								text: "stopped - switched to manual, flip back to auto to resume",
							} satisfies TranscriptItem,
						]
					: chat.transcript,
		}));
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
				applyChats((current) => removeEntry(current, prevKey));
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

	async function handleAnswer(
		session: SessionKey,
		toolCallId: string,
		optionId: string,
	) {
		if (readOnly) return;
		await answerPermission(session, toolCallId, optionId);
		updateChat(session, (chat) =>
			markAcked({ ...chat, approval: false }, Date.now()),
		);
	}

	async function handleConfigChange(configId: string, value: string) {
		configGeneration.current += 1;
		const generation = configGeneration.current;
		try {
			const options = await setGlobalConfigOption(configId, value);
			if (configGeneration.current !== generation) return;
			applyChats((current) => {
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
				updateChat(key, (chat) => markAcked(chat, Date.now()));
			}
		} catch (error) {
			updateChat(key, (chat) => markIdle(chat));
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
		if (view.kind === "picker") return;
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
				applyChats((chats) => removeEntry(chats, vanished));
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

	handleNewPlanRef.current = () => void handleNewPlan();

	async function handleExecute(key: SessionKey) {
		updateChat(key, (chat) => markSent(chat));
		try {
			const update = await executePlan(key);
			const isViewed = selectedRef.current?.plan === key.plan;
			applyChats((current) =>
				carryHistory(current, key, update, isViewed, Date.now()),
			);
			applyOwnedUpdate(key.plan, update);
		} catch (error) {
			updateChat(key, (chat) => markIdle(chat));
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
		updateChat(key, (chat) => markSent(chat));
		try {
			const update = await cancelPlan(key);
			if (wasEmpty) {
				applyChats((current) => removeEntry(current, key));
				draft.clearDraft(key);
			} else {
				applyChats((current) =>
					carryHistory(current, key, update, false, Date.now()),
				);
			}
			applyOwnedUpdate(key.plan, update);
		} catch (error) {
			updateChat(key, (chat) => markIdle(chat));
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
			applyOwnedUpdate(plan, update);
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
			applyOwnedUpdate(plan, update);
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
	const openPaths = useMemo(
		() => new Set(openRepos.map((repo) => repo.path)),
		[openRepos],
	);

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
					openPaths={openPaths}
					onOpen={handleOpenPath}
					onBrowse={handleBrowse}
					onDismissError={() => setPickerError(null)}
				/>
			) : (
				<div className="mainrow">
					<Sidebar
						mode={sidebarMode}
						repoLabel={repoLabel}
						repoRoot={repoRoot}
						branch={branch}
					>
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
					</Sidebar>
					<div className="chatcol">
						<div className="transcript" ref={scrollRef} onScroll={onScroll}>
							<Transcript
								items={transcript}
								start={chat?.start ?? null}
								repoLabel={repoLabel}
								onRetry={readOnly ? null : handleRetry}
								onAnswer={handleAnswer}
								turn={liveTurnOf(chat)}
								onStop={readOnly ? null : handleStop}
								session={selectedKey}
								phase={entry?.phase ?? null}
								onHistoryRetry={
									chat?.start.kind === "failed" && chat.start.error.retryable
										? handleHistoryRetry
										: null
								}
							>
								{selectedKey !== null && !readOnly && (
									<>
										<QueuedBubbleList
											items={queue.items}
											editingId={queue.editingId}
											editingBlocked={editingBlocked}
											session={selectedKey}
											onEdit={handleQueueEdit}
											onCommit={handleQueueCommit}
											onCancel={handleQueueCancel}
											onMove={handleQueueMove}
											onAttachmentError={handleAttachmentError}
										/>
										<DraftBubble
											key={selectedId}
											initialText={draft.initialText}
											placeholder={
												busy ? "Queue a follow-up…" : "Ask for a change…"
											}
											session={selectedKey}
											contextPct={draftContextPct}
											onSend={handleSend}
											onEdit={notifyEdit}
											onInput={draft.onDraftInput}
											onAttachmentError={handleAttachmentError}
										/>
									</>
								)}
								{readOnly && (
									<div className="ro-note">
										This session is read-only - Plan is {entry?.phase}.
									</div>
								)}
							</Transcript>
						</div>
					</div>
					{isScoping ? (
						<PlanMdPane text={planMd} />
					) : side.kind === "commits" ? (
						<CommitsPanel
							commits={side.commits}
							done={side.done}
							total={side.total}
							working={side.working}
						/>
					) : (
						<SidePanel todos={side.todos} />
					)}
				</div>
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

// The backend's single-owner rejection. The backend focuses the owning
// window itself; the picker only shows a notice and stays put.
function isAlreadyOpenError(error: unknown): boolean {
	const message = error instanceof Error ? error.message : String(error);
	return message.startsWith("already open");
}
