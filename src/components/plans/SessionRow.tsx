import type {
	PlanPhase,
	SessionKey,
	SessionStatusView,
	WorktreeStatus,
} from "../../types";
import { type AttentionKind, roleLabel } from "./planDisplay";
import { actionKind, RowActions } from "./RowActions";

export function SessionRow({
	planName,
	planTitle,
	phase,
	hasPlanMd,
	worktree,
	status,
	openPath,
	trail,
	position,
	onSelect,
	onExecute,
	onCancel,
	onFinishLanding,
	onBeginLanding,
}: {
	planName: string;
	planTitle: string;
	phase: PlanPhase;
	hasPlanMd: boolean;
	worktree: WorktreeStatus | null;
	status: SessionStatusView;
	openPath: AttentionKind | "sel";
	trail: "before" | "open" | "after";
	position: "single" | "last" | "middle";
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onFinishLanding: (session: SessionKey) => void;
	onBeginLanding: (session: SessionKey) => void;
}) {
	const key: SessionKey = { plan: planName, role: status.role };
	const kind = actionKind(phase, status.role);
	return (
		<>
			<span className={gutterClass(position)} aria-hidden="true">
				<i className={`v${spineClass(openPath, trail, position)}`} />
				<i className={`h${nodeClass(openPath, trail)}`} />
				<i className={`nd${nodeClass(openPath, trail)}`} />
			</span>
			<div className={`sbody${trail === "open" ? " selected" : ""}`}>
				<button
					className="srow"
					type="button"
					onClick={() => onSelect(key)}
					aria-label={`${planTitle} ${roleLabel(status.role)}`}
				>
					<span className="slabel">{roleLabel(status.role)}</span>
				</button>
				{kind === "scoping" ? (
					<RowActions
						kind={kind}
						planName={planName}
						hasPlanMd={hasPlanMd}
						running={status.working}
						onExecute={() => onExecute(key)}
						onCancel={() => onCancel(key)}
					/>
				) : kind === "executing" ? (
					<RowActions
						kind={kind}
						planName={planName}
						running={status.working}
						worktree={worktree}
						onCancel={() => onCancel(key)}
						onFinishLanding={() => onFinishLanding(key)}
						onBeginLanding={() => onBeginLanding(key)}
					/>
				) : kind === "landing" ? (
					<RowActions
						kind={kind}
						planName={planName}
						running={status.working}
						dirty={worktree?.dirty ?? false}
						onCancel={() => onCancel(key)}
						onFinishLanding={() => onFinishLanding(key)}
					/>
				) : (
					<RowActions kind={kind} planName={planName} />
				)}
			</div>
		</>
	);
}

function gutterClass(position: "single" | "last" | "middle"): string {
	switch (position) {
		case "single":
			return "ngutter single";
		case "last":
			return "ngutter last";
		case "middle":
			return "ngutter";
	}
}

function spineClass(
	openPath: AttentionKind | "sel",
	trail: "before" | "open" | "after",
	position: "single" | "last" | "middle",
): string {
	switch (trail) {
		case "before":
			return ` c-${openPath}`;
		case "open":
			return position === "middle" ? ` u-${openPath}` : ` c-${openPath}`;
		case "after":
			return "";
	}
}

function nodeClass(
	openPath: AttentionKind | "sel",
	trail: "before" | "open" | "after",
): string {
	if (trail !== "open") {
		return "";
	}
	return openPath === "sel" ? " sel" : ` c-${openPath}`;
}
