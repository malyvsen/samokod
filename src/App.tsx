import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
	abandonPlan,
	answerPermission,
	cancelTurn,
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
import { RepoPicker } from "./components/RepoPicker";
import { SidePanel } from "./components/SidePanel";
import { TopBar } from "./components/TopBar";
import { Transcript } from "./components/Transcript";
import type {
	AgentStatus,
	AppEvent,
	PlanEntry,
	PlanInfo,
	PlansUpdate,
	RecentRepo,
	SessionInfo,
	SessionKey,
	SpendView,
	TodoView,
	TranscriptItem,
} from "./types";
import { sameSession } from "./types";
import "./App.css";

type View = { kind: "picker"; returnToChat: boolean } | { kind: "chat" };

export function App() {
	const [view, setView] = useState<View>({
		kind: "picker",
		returnToChat: false,
	});
	const [session, setSession] = useState<SessionInfo | null>(null);
	const [recent, setRecent] = useState<RecentRepo[]>([]);
	const [pickerError, setPickerError] = useState<string | null>(null);
	const [transcript, setTranscript] = useState<TranscriptItem[]>([]);
	const [todos, setTodos] = useState<TodoView[]>([]);
	const [spend, setSpend] = useState<SpendView | null>(null);
	const [plan, setPlan] = useState<PlanInfo | null>(null);
	const [, setPlans] = useState<PlanEntry[]>([]);
	const [selectedKey, setSelectedKey] = useState<SessionKey | null>(null);
	const [working, setWorking] = useState(false);
	const [awaitingApproval, setAwaitingApproval] = useState(false);
	const [failed, setFailed] = useState(false);
	const appRef = useRef<HTMLDivElement>(null);
	const notifyEdit = useAuroraMotion(appRef);
	const transcriptRef = useRef<HTMLDivElement>(null);
	const configGeneration = useRef(0);

	const status: AgentStatus = awaitingApproval
		? "approval"
		: working
			? "working"
			: failed
				? "failed"
				: "idle";

	const selectedRef = useRef<SessionKey | null>(null);
	selectedRef.current = selectedKey;

	const applyPlans = useCallback((update: PlansUpdate) => {
		setPlans(update.plans);
		setSelectedKey(update.selected);
		setPlan(planOfSelected(update.plans, update.selected));
	}, []);

	const agentLabel = agentLabelForPlan(plan);

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
				setSession((current) =>
					current === null ? current : { ...current, branch: event.branch },
				);
				return;
			}
			// Background sessions update silently; only the selected
			// session renders.
			if (!sameSession(event.session, selectedRef.current)) return;
			switch (event.type) {
				case "agent_text": {
					setWorking(true);
					setFailed(false);
					setTranscript((items) => {
						const last = items[items.length - 1];
						if (last !== undefined && last.kind === "agent") {
							return [
								...items.slice(0, -1),
								{ ...last, text: last.text + event.chunk },
							];
						}
						return [
							...items,
							{ kind: "agent", id: crypto.randomUUID(), text: event.chunk },
						];
					});
					break;
				}
				case "tool_line": {
					setWorking(true);
					setFailed(false);
					setTranscript((items) => {
						const index = items.findIndex(
							(item) => item.kind === "tool" && item.line.id === event.line.id,
						);
						if (index >= 0) {
							const copy = [...items];
							copy[index] = {
								kind: "tool",
								id: copy[index]?.id ?? crypto.randomUUID(),
								line: event.line,
							};
							return copy;
						}
						return [
							...items,
							{ kind: "tool", id: crypto.randomUUID(), line: event.line },
						];
					});
					break;
				}
				case "turn_done": {
					setWorking(false);
					setAwaitingApproval(false);
					setFailed(false);
					break;
				}
				case "turn_failed":
				case "agent_exited": {
					setWorking(false);
					setAwaitingApproval(false);
					setFailed(true);
					setTranscript((items) => [
						...items,
						failureItem(event.raw, event.hint, event.retryable),
					]);
					break;
				}
				case "permission_asked": {
					setAwaitingApproval(true);
					setTranscript((items) => [
						...items,
						{
							kind: "approval",
							id: crypto.randomUUID(),
							permission: event.permission,
							resolved: false,
						},
					]);
					break;
				}
				case "permission_resolved": {
					setTranscript((items) =>
						items.map((item) =>
							item.kind === "approval" &&
							item.permission.tool_call_id === event.tool_call_id
								? { ...item, resolved: true }
								: item,
						),
					);
					break;
				}
				case "config_options": {
					setSession((current) =>
						current === null
							? current
							: { ...current, config_options: event.options },
					);
					break;
				}
				case "todos_changed": {
					setTodos(event.todos);
					if (event.changes.length > 0) {
						setTranscript((items) => [
							...items,
							{
								kind: "todos",
								id: crypto.randomUUID(),
								changes: event.changes,
							},
						]);
					}
					break;
				}
				case "spend_tick": {
					setSpend({
						cost: event.cost,
						contextPct: event.ctx_pct,
					});
					break;
				}
				case "session_reset": {
					setTodos([]);
					setSpend(null);
					break;
				}
				case "plan_changed": {
					setPlan(event.plan);
					break;
				}
			}
		},
		[applyPlans],
	);

	useEffect(() => onAppEvent(handleEvent), [handleEvent]);

	useEffect(() => {
		if (view.kind !== "chat") return;
		function onFocus() {
			refreshBranch()
				.then((branch) => {
					setSession((current) =>
						current === null ? current : { ...current, branch },
					);
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

	// biome-ignore lint/correctness/useExhaustiveDependencies: re-scroll whenever the transcript identity changes
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
			if (info.root === session?.repo_root) {
				setView({ kind: "chat" });
				return;
			}
			const opened = await openRepo(info.root);
			applyPlans({ plans: opened.plans, selected: opened.selected });
			setSession({
				session_id: "",
				repo_root: opened.repo_root,
				branch: opened.branch,
				config_options: [],
				plan: planOfSelected(opened.plans, opened.selected),
			});
			setTranscript([]);
			setTodos([]);
			setSpend(null);
			setWorking(false);
			setAwaitingApproval(false);
			setFailed(false);
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

	function appendFailure(raw: string, hint: string, retryable: boolean) {
		setTranscript((items) => [...items, failureItem(raw, hint, retryable)]);
	}

	function appendError(raw: string) {
		appendFailure(raw, "retry the turn", true);
	}

	async function runTurn(text: string) {
		const key = selectedRef.current;
		if (key === null) return;
		setWorking(true);
		setFailed(false);
		try {
			await sendPrompt(key, text);
		} catch (error) {
			setWorking(false);
			appendError(error instanceof Error ? error.message : String(error));
		}
	}

	async function handleSend(text: string) {
		if (text === "" || working || awaitingApproval || session === null) return;
		setTranscript((items) => [
			...items,
			{ kind: "user", id: crypto.randomUUID(), text },
		]);
		await runTurn(text);
	}

	async function handleStop() {
		const key = selectedRef.current;
		if (key === null) return;
		try {
			await cancelTurn(key);
		} finally {
			setWorking(false);
			setAwaitingApproval(false);
		}
	}

	async function handleAnswer(toolCallId: string, optionId: string) {
		const key = selectedRef.current;
		if (key === null) return;
		await answerPermission(key, toolCallId, optionId);
		setAwaitingApproval(false);
		setWorking(true);
	}

	async function handleConfigChange(configId: string, value: string) {
		const key = selectedRef.current;
		if (working || awaitingApproval || key === null) return;
		configGeneration.current += 1;
		const generation = configGeneration.current;
		try {
			const options = await setConfigOption(key, configId, value);
			if (configGeneration.current !== generation) return;
			setSession((current) =>
				current === null ? current : { ...current, config_options: options },
			);
		} catch (error) {
			console.warn(`set_config_option ${configId}=${value} failed`, error);
			return;
		}
	}

	async function handleRetry() {
		const key = selectedRef.current;
		if (working || awaitingApproval || key === null) return;
		setFailed(false);
		try {
			const retried = await retryLast(key);
			if (retried) setWorking(true);
		} catch (error) {
			appendError(error instanceof Error ? error.message : String(error));
		}
	}

	function handleRepoButton() {
		if (working || awaitingApproval) return;
		setView({ kind: "picker", returnToChat: true });
	}

	async function handleExecute() {
		const key = selectedRef.current;
		if (working || awaitingApproval || key === null) return;
		setWorking(true);
		try {
			const update = await executePlan(key);
			applyPlans(update);
			// The executor turn it just started is already running.
			setWorking(true);
			setTranscript([]);
			setTodos([]);
			setSpend(null);
		} catch (error) {
			setWorking(false);
			appendError(error instanceof Error ? error.message : String(error));
		}
	}

	async function handleComplete() {
		await runPlansAction((session) => markCompleted(session));
	}

	async function handleAbandon() {
		await runPlansAction((session) => abandonPlan(session));
	}

	async function runPlansAction(
		action: (session: SessionKey) => Promise<PlansUpdate>,
	) {
		const key = selectedRef.current;
		if (working || awaitingApproval || key === null) return;
		setWorking(true);
		try {
			applyPlans(await action(key));
			setTranscript([]);
			setTodos([]);
			setSpend(null);
			setWorking(false);
		} catch (error) {
			setWorking(false);
			appendError(error instanceof Error ? error.message : String(error));
		}
	}

	const repoLabel = session === null ? "no repo" : shortPath(session.repo_root);
	const branch = session?.branch ?? "HEAD";
	const configOptions = session?.config_options ?? [];
	const returnToChat = view.kind === "picker" && view.returnToChat;

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
					subtitle={
						returnToChat
							? "switch repository - the current chat closes"
							: "open a git repository to start one chat"
					}
					recent={recent}
					currentPath={returnToChat ? (session?.repo_root ?? null) : null}
					error={pickerError}
					onOpen={handleOpenPath}
					onBrowse={handleBrowse}
					onBack={returnToChat ? () => setView({ kind: "chat" }) : null}
					onDismissError={() => setPickerError(null)}
				/>
			) : (
				<>
					<TopBar
						repoLabel={repoLabel}
						branch={branch}
						status={status}
						plan={plan}
						onOpenPicker={handleRepoButton}
						onStop={handleStop}
						onExecute={handleExecute}
						onComplete={handleComplete}
						onAbandon={handleAbandon}
					/>
					<div className="mainrow">
						<div className="chatcol">
							<div className="transcript" ref={transcriptRef}>
								<Transcript
									items={transcript}
									repoLabel={repoLabel}
									agentLabel={agentLabel}
									onRetry={handleRetry}
									onAnswer={handleAnswer}
								>
									{status === "idle" && (
										<DraftBubble onSend={handleSend} onEdit={notifyEdit} />
									)}
								</Transcript>
							</div>
						</div>
						<SidePanel
							todos={todos}
							spend={spend}
							sessionId={session?.session_id ?? ""}
							options={configOptions}
							disabled={status !== "idle"}
							onChange={handleConfigChange}
						/>
					</div>
				</>
			)}
		</div>
	);
}

function failureItem(
	raw: string,
	hint: string,
	retryable: boolean,
): TranscriptItem {
	return { kind: "error", id: crypto.randomUUID(), raw, hint, retryable };
}

function planOfSelected(plans: PlanEntry[], selected: SessionKey): PlanInfo {
	const entry = plans.find((plan) => plan.name === selected.plan);
	return {
		name: selected.plan,
		phase: entry?.phase === "scoping" ? "scoping" : "executing",
		has_plan_md: true,
		title: entry?.title ?? "Untitled",
	};
}

function agentLabelForPlan(plan: PlanInfo | null): string {
	if (plan === null) return "AGENT";
	switch (plan.phase) {
		case "scoping":
			return "PLANNER";
		case "executing":
			return "EXECUTOR";
		case "completed":
		case "cancelled":
			return "AGENT";
	}
}

function shortPath(path: string): string {
	const home = typeof process !== "undefined" ? process.env.HOME : undefined;
	if (home !== undefined && home !== "" && path.startsWith(`${home}/`)) {
		return `~/${path.slice(home.length + 1)}`;
	}
	return path;
}
