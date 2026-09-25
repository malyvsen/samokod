import { useState } from "react";
import type { AgentStatus, PlanInfo, PlanPhase } from "../types";

const STATUS: Record<AgentStatus, { text: string; className: string }> = {
	idle: { text: "IDLE", className: "status" },
	working: { text: "● WORKING", className: "status live" },
	approval: { text: "● PAUSED - APPROVAL", className: "status paused" },
	failed: { text: "● FAILED", className: "status failed" },
};

export function TopBar({
	repoLabel,
	branch,
	status,
	plan,
	onOpenPicker,
	onStop,
	onExecute,
	onComplete,
	onAbandon,
}: {
	repoLabel: string;
	branch: string;
	status: AgentStatus;
	plan: PlanInfo | null;
	onOpenPicker: () => void;
	onStop: () => void;
	onExecute: () => void;
	onComplete: () => void;
	onAbandon: () => void;
}) {
	const busy = status === "working" || status === "approval";
	const { text, className } = STATUS[status];
	const tip = busy
		? "stop the agent to switch repositories"
		: "open repo picker";
	// Remount on swap so the pulse animation leaves no stale paint in the WebView compositor.
	const pill = (
		<span key={status} className={className}>
			{text}
		</span>
	);
	return (
		<div className="topbar">
			<span className="brand">SAMOKOD</span>
			<button
				className="tbtn repo-btn"
				type="button"
				data-tip={tip}
				onClick={onOpenPicker}
			>
				<b>{repoLabel}</b> · {branch}
			</button>
			<PlanMenu
				plan={plan}
				disabled={busy}
				onExecute={onExecute}
				onComplete={onComplete}
				onAbandon={onAbandon}
			/>
			{busy ? (
				<span className="stop-wrap">
					{pill}
					<button className="stop-ctl" type="button" onClick={onStop}>
						<span className="sq" aria-hidden="true" />
						STOP
					</button>
				</span>
			) : (
				pill
			)}
		</div>
	);
}

const PHASE_LABEL: Record<PlanPhase, string> = {
	scoping: "SCOPING",
	executing: "EXECUTING",
	completed: "COMPLETED",
	cancelled: "CANCELLED",
};

interface PlanOption {
	label: string;
	tip?: string | undefined;
	disabled: boolean;
	danger?: boolean;
	onSelect: () => void;
}

function PlanMenu({
	plan,
	disabled,
	onExecute,
	onComplete,
	onAbandon,
}: {
	plan: PlanInfo | null;
	disabled: boolean;
	onExecute: () => void;
	onComplete: () => void;
	onAbandon: () => void;
}) {
	const [open, setOpen] = useState(false);
	if (plan === null) return null;
	const lead: PlanOption = (() => {
		switch (plan.phase) {
			case "scoping":
				return {
					label: "Execute",
					tip: plan.has_plan_md ? undefined : "Needs plan.md",
					disabled: !plan.has_plan_md,
					onSelect: onExecute,
				} satisfies PlanOption;
			case "executing":
				return {
					label: "Mark completed",
					disabled: false,
					onSelect: onComplete,
				} satisfies PlanOption;
			case "completed":
			case "cancelled":
				return {
					label: "Mark completed",
					disabled: true,
					onSelect: onComplete,
				} satisfies PlanOption;
		}
	})();
	const options: PlanOption[] = [
		lead,
		{
			label: "Abandon",
			disabled: false,
			danger: true,
			onSelect: onAbandon,
		},
	];
	return (
		<span className="dwrap">
			<button
				className={`dsel plan-${plan.phase}`}
				type="button"
				disabled={disabled}
				onClick={() => setOpen((value) => !value)}
				aria-label={`plan phase ${plan.phase}`}
			>
				<span className="dlabel">{PHASE_LABEL[plan.phase]}</span>
				<span className="arrow">▾</span>
			</button>
			{open && !disabled && (
				<span className="dpop">
					{options.map((option) => (
						<button
							className={`dopt${option.danger ? " danger" : ""}`}
							key={option.label}
							type="button"
							disabled={option.disabled}
							aria-label={
								option.tip === undefined
									? option.label
									: `${option.label} ${option.tip}`
							}
							onClick={() => {
								option.onSelect();
								setOpen(false);
							}}
						>
							<span>{option.label}</span>
							{option.tip !== undefined && (
								<span className="sub">{option.tip}</span>
							)}
						</button>
					))}
				</span>
			)}
		</span>
	);
}
