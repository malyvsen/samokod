import { useEffect, useRef, useState } from "react";
import type { PlanEntry, SessionKey } from "../../types";

export function PlanMenu({
	plan,
	target,
	onExecute,
	onCancel,
	onSetMode,
	onSetEvergreen,
}: {
	plan: PlanEntry;
	target: SessionKey;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onSetMode: (plan: string, manual: boolean) => void;
	onSetEvergreen: (plan: string, evergreen: boolean) => void;
}) {
	const [open, setOpen] = useState(false);
	const rootRef = useRef<HTMLSpanElement>(null);

	useEffect(() => {
		if (!open) return;
		function onPointerDown(event: PointerEvent) {
			if (rootRef.current?.contains(event.target as Node)) return;
			setOpen(false);
		}
		function onKeyDown(event: KeyboardEvent) {
			if (event.key === "Escape") setOpen(false);
		}
		document.addEventListener("pointerdown", onPointerDown);
		document.addEventListener("keydown", onKeyDown);
		return () => {
			document.removeEventListener("pointerdown", onPointerDown);
			document.removeEventListener("keydown", onKeyDown);
		};
	}, [open]);

	const scopingWorking =
		plan.sessions.find((status) => status.role === "scoping")?.working ?? false;
	const paused = plan.manual;
	const everOn = plan.evergreen;

	return (
		<span className="hact" ref={rootRef}>
			<button
				className="mctl"
				type="button"
				aria-label={`Plan options for ${plan.title}`}
				aria-expanded={open}
				onClick={(event) => {
					event.stopPropagation();
					setOpen((was) => !was);
				}}
			>
				...
			</button>
			{open ? (
				<span className="pdrop" role="menu">
					{plan.phase === "scoping" ? (
						<>
							<button
								className={`dopt${everOn ? " on" : ""}`}
								type="button"
								role="menuitemcheckbox"
								aria-checked={everOn}
								onClick={() => onSetEvergreen(plan.name, !everOn)}
								title={
									everOn
										? "Will clean after executing. Click to go straight to landing instead."
										: "Skipping cleanup. Click to clean after executing."
								}
							>
								<span className="mark" aria-hidden="true">
									{everOn ? "[x]" : "[ ]"}
								</span>
								Evergreen
							</button>
							<div className="dsep" />
							<button
								className="dopt"
								type="button"
								role="menuitem"
								disabled={scopingWorking || !plan.has_plan_md}
								onClick={() => {
									setOpen(false);
									onExecute(target);
								}}
								title="Move to executing and start the agent"
							>
								<span className="mark" aria-hidden="true" />
								Execute
							</button>
						</>
					) : (
						<>
							<button
								className={`dopt${paused ? "" : " on"}`}
								type="button"
								role="menuitemcheckbox"
								aria-checked={!paused}
								onClick={() => onSetMode(plan.name, !paused)}
								title={
									paused
										? "Paused - awaits approval. Click to let it advance on its own."
										: "Advancing on its own. Click to pause and await approval."
								}
							>
								<span className="mark" aria-hidden="true">
									{paused ? "[ ]" : "[x]"}
								</span>
								Auto-advance
							</button>
							{plan.phase === "executing" ? (
								<button
									className={`dopt${everOn ? " on" : ""}`}
									type="button"
									role="menuitemcheckbox"
									aria-checked={everOn}
									onClick={() => onSetEvergreen(plan.name, !everOn)}
									title={
										everOn
											? "Will clean after executing. Click to go straight to landing instead."
											: "Skipping cleanup. Click to clean after executing."
									}
								>
									<span className="mark" aria-hidden="true">
										{everOn ? "[x]" : "[ ]"}
									</span>
									Evergreen
								</button>
							) : null}
							<div className="dsep" />
						</>
					)}
					<button
						className="dopt danger"
						type="button"
						role="menuitem"
						onClick={() => {
							setOpen(false);
							onCancel(target);
						}}
						title="Move to cancelled, keeps history"
					>
						<span className="mark" aria-hidden="true" />
						Cancel
					</button>
				</span>
			) : null}
		</span>
	);
}
