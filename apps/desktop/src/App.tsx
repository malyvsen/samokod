import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import { getPrefs, openRepo, validateRepo } from "./api";
import { Composer } from "./components/Composer";
import { RepoPicker } from "./components/RepoPicker";
import { TopBar } from "./components/TopBar";
import type { Prefs, RecentRepo, SessionInfo } from "./types";
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
	const [draft, setDraft] = useState("");
	const [typing, setTyping] = useState(false);
	const typeTimer = useRef<number | null>(null);
	const appRef = useRef<HTMLDivElement>(null);

	const applySession = useCallback((info: SessionInfo) => {
		setSession(info);
		setDraft("");
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
			data-state="idle"
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
						working={false}
						statusText="IDLE"
						onOpenPicker={() => {
							setView({ kind: "picker", returnToChat: true });
						}}
						onNewChat={() => undefined}
					/>
					<div className="transcript">
						<div className="empty-hint">
							<b>{repoLabel} · fresh session</b>
							no messages yet
						</div>
					</div>
					<Composer
						draft={draft}
						configOptions={configOptions}
						wired={false}
						onDraft={setDraft}
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
