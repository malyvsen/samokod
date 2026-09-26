import type {
	PlanPhase,
	SessionKey,
	SessionStatusView,
	WorktreeStatus,
} from "../../types";
import { sameSession } from "../../types";
import { actionKind, RowActions } from "./RowActions";
import { dotClass, sessionLabel } from "./sessionDot";

export function SessionRow({
	planName,
	planTitle,
	phase,
	hasPlanMd,
	worktree,
	status,
	selected,
	onSelect,
	onExecute,
	onAbandon,
	onCancel,
	onDone,
	onBeginMerge,
}: {
	planName: string;
	planTitle: string;
	phase: PlanPhase;
	hasPlanMd: boolean;
	worktree: WorktreeStatus | null;
	status: SessionStatusView;
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onAbandon: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onDone: (session: SessionKey) => void;
	onBeginMerge: (session: SessionKey) => void;
}) {
	const key: SessionKey = { plan: planName, role: status.role };
	const isSelected = sameSession(selected, key);
	const dot = dotClass(phase, status.role, status);
	const kind = actionKind(phase, status.role);
	return (
		<div className={`session${isSelected ? " selected" : ""}`}>
			<button
				className="srow"
				type="button"
				onClick={() => onSelect(key)}
				aria-label={`${planTitle} ${sessionLabel(status.role)}`}
			>
				<span className={`dot ${dot}`} aria-hidden="true" />
				<span className="slabel">{sessionLabel(status.role)}</span>
			</button>
			{kind === "scoping" ? (
				<RowActions
					kind={kind}
					planName={planName}
					hasPlanMd={hasPlanMd}
					running={status.working}
					onExecute={() => onExecute(key)}
					onAbandon={() => onAbandon(key)}
				/>
			) : kind === "executing" ? (
				<RowActions
					kind={kind}
					planName={planName}
					running={status.working}
					worktree={worktree}
					onCancel={() => onCancel(key)}
					onDone={() => onDone(key)}
					onBeginMerge={() => onBeginMerge(key)}
				/>
			) : kind === "merging" ? (
				<RowActions
					kind={kind}
					planName={planName}
					running={status.working}
					dirty={worktree?.dirty ?? false}
					onCancel={() => onCancel(key)}
					onDone={() => onDone(key)}
				/>
			) : (
				<RowActions kind={kind} planName={planName} />
			)}
		</div>
	);
}
