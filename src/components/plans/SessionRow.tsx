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
	isOpen,
	isBeforeOpen,
	isLast,
	isSingle,
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
	openPath: AttentionKind | "neutral";
	isOpen: boolean;
	isBeforeOpen: boolean;
	isLast: boolean;
	isSingle: boolean;
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
			<span className={gutterClass(isLast, isSingle)} aria-hidden="true">
				<i
					className={`v${spineClass(openPath, isOpen, isBeforeOpen, isLast)}`}
				/>
				<i className={`h${nodeClass(openPath, isOpen)}`} />
				<i className={`nd${nodeClass(openPath, isOpen)}`} />
			</span>
			<div className={`sbody${isOpen ? " selected" : ""}`}>
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

function gutterClass(isLast: boolean, isSingle: boolean): string {
	if (isSingle) {
		return "ngutter single";
	}
	if (isLast) {
		return "ngutter last";
	}
	return "ngutter";
}

function pathKey(openPath: AttentionKind | "neutral"): string {
	return openPath === "neutral" ? "sel" : openPath;
}

function spineClass(
	openPath: AttentionKind | "neutral",
	isOpen: boolean,
	isBeforeOpen: boolean,
	isLast: boolean,
): string {
	if (isBeforeOpen) {
		return ` c-${pathKey(openPath)}`;
	}
	if (isOpen) {
		return isLast ? ` c-${pathKey(openPath)}` : ` u-${pathKey(openPath)}`;
	}
	return "";
}

function nodeClass(
	openPath: AttentionKind | "neutral",
	isOpen: boolean,
): string {
	if (!isOpen) {
		return "";
	}
	return openPath === "neutral" ? " sel" : ` c-${pathKey(openPath)}`;
}
