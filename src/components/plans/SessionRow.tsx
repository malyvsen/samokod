import type {
	PlanPhase,
	SessionKey,
	SessionStatusView,
	WorktreeStatus,
} from "../../types";
import { sameSession } from "../../types";
import { roleLabel } from "./planDisplay";
import { actionKind, RowActions } from "./RowActions";

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
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onFinishLanding: (session: SessionKey) => void;
	onBeginLanding: (session: SessionKey) => void;
}) {
	const key: SessionKey = { plan: planName, role: status.role };
	const isSelected = sameSession(selected, key);
	const kind = actionKind(phase, status.role);
	return (
		<div className={`session${isSelected ? " selected" : ""}`}>
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
	);
}
