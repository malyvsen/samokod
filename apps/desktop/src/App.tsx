import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import {
	answerPermission,
	cancelTurn,
	getPrefs,
	onAppEvent,
	openRepo,
	sendPrompt,
	setConfigOption,
	validateRepo,
} from "./api";
import { Composer } from "./components/Composer";
import { RepoPicker } from "./components/RepoPicker";
import { TopBar } from "./components/TopBar";
import { Transcript } from "./components/Transcript";
import type {
	AgentStatus,
	AppEvent,
	Prefs,
	RecentRepo,
	SessionInfo,
	TranscriptItem,
} from "./types";
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
	const [working, setWorking] = useState(false);
	const [awaitingApproval, setAwaitingApproval] = useState(false);
	const [draft, setDraft] = useState("");
	const [lastSent, setLastSent] = useState("");
	const [typing, setTyping] = useState(false);
	const typeTimer = useRef<number | null>(null);
	const appRef = useRef<HTMLDivElement>(null);
	const transcriptRef = useRef<HTMLDivElement>(null);

	const status: AgentStatus = awaitingApproval
		? "approval"
		: working
			? "working"
			: "idle";
	const statusText =
		status === "idle"
			? "IDLE"
			: status === "working"
				? "● WORKING"
				: "● PAUSED - APPROVAL";

	const applySession = useCallback((info: SessionInfo) => {
		setSession(info);
		setTranscript([]);
		setWorking(false);
		setAwaitingApproval(false);
		setDraft("");
		setLastSent("");
		setView({ kind: "chat" });
		setPickerError(null);
	}, []);

	useEffect(() => {
		let cancelled = false;
		getPrefs()
			.then(async (prefs: Prefs) => {
				if (cancelled) return;
				setRecent(prefs.recent);
				if (prefs.last_repo != null && prefs.last_repo !== "") {
					try {
						const info = await validateRepo(prefs.last_repo);
						if (cancelled) return;
						const opened = await openRepo(info.root);
						if (cancelled) return;
						applySession(opened);
						setRecent((await getPrefs()).recent);
					} catch {
						if (!cancelled) setView({ kind: "picker", returnToChat: false });
					}
				}
			})
			.catch(() => {
				if (!cancelled) setView({ kind: "picker", returnToChat: false });
			});
		return () => {
			cancelled = true;
		};
	}, [applySession]);

	const handleEvent = useCallback((event: AppEvent) => {
		switch (event.type) {
			case "agent_text": {
				setWorking(true);
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
				break;
			}
			case "turn_failed": {
				setWorking(false);
				setAwaitingApproval(false);
				setTranscript((items) => [
					...items,
					{
						kind: "error",
						id: crypto.randomUUID(),
						raw: event.raw,
					},
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
		}
	}, []);

	useEffect(() => {
		let unlisten: (() => void) | undefined;
		onAppEvent(handleEvent).then((stop) => {
			unlisten = stop;
		});
		return () => {
			unlisten?.();
		};
	}, [handleEvent]);

	// biome-ignore lint/correctness/useExhaustiveDependencies: re-scroll whenever the transcript identity changes
	useEffect(() => {
		const node = transcriptRef.current;
		if (node !== null) {
			node.scrollTop = node.scrollHeight;
		}
	}, [transcript]);

	useEffect(() => {
		function onPointerMove(event: MouseEvent) {
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
			applySession(opened);
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

	function appendError(raw: string) {
		setTranscript((items) => [
			...items,
			{ kind: "error", id: crypto.randomUUID(), raw },
		]);
	}

	async function runTurn(text: string) {
		setWorking(true);
		try {
			await sendPrompt(text);
		} catch (error) {
			setWorking(false);
			appendError(error instanceof Error ? error.message : String(error));
		}
	}

	async function handleSend() {
		const text = draft.trim();
		if (text === "" || status !== "idle" || session === null) return;
		setTranscript((items) => [
			...items,
			{ kind: "user", id: crypto.randomUUID(), text },
		]);
		setDraft("");
		setLastSent(text);
		await runTurn(text);
	}

	async function handleStop() {
		try {
			await cancelTurn();
		} finally {
			setWorking(false);
			setAwaitingApproval(false);
		}
	}

	async function handleAnswer(toolCallId: string, optionId: string) {
		await answerPermission(toolCallId, optionId);
		setAwaitingApproval(false);
		setWorking(true);
	}

	async function handleConfigChange(id: string, value: string) {
		if (status !== "idle") return;
		await setConfigOption(id, value);
		setSession((current) =>
			current === null
				? current
				: {
						...current,
						config_options: current.config_options.map((option) =>
							option.id === id ? { ...option, current: value } : option,
						),
					},
		);
	}

	async function handleRetry() {
		if (lastSent === "" || working) return;
		await runTurn(lastSent);
	}

	function handleRepoButton() {
		if (status !== "idle") return;
		setView({ kind: "picker", returnToChat: true });
	}

	function handleTypePulse() {
		setTyping(true);
		if (typeTimer.current !== null) window.clearTimeout(typeTimer.current);
		typeTimer.current = window.setTimeout(() => setTyping(false), 450);
	}

	const repoLabel = session === null ? "no repo" : shortPath(session.repo_root);
	const branch = session?.branch ?? "HEAD";
	const configOptions = session?.config_options ?? [];
	const returnToChat = view.kind === "picker" && view.returnToChat;

	return (
		<div
			className={`app${typing ? " typing" : ""}`}
			data-state={status}
			ref={appRef}
		>
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
						working={status !== "idle"}
						statusText={statusText}
						onOpenPicker={handleRepoButton}
						onNewChat={() => undefined}
					/>
					<div className="transcript" ref={transcriptRef}>
						{transcript.length === 0 ? (
							<div className="empty-hint">
								<b>{repoLabel} · fresh session</b>
								no messages yet
							</div>
						) : (
							<Transcript
								items={transcript}
								onRetry={handleRetry}
								onAnswer={handleAnswer}
							/>
						)}
					</div>
					<Composer
						status={status}
						draft={draft}
						configOptions={configOptions}
						onDraft={setDraft}
						onSend={handleSend}
						onStop={handleStop}
						onConfigChange={handleConfigChange}
						onTypePulse={handleTypePulse}
					/>
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
