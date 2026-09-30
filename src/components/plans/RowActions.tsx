import type { PlanPhase, SessionRole, WorktreeStatus } from "../../types";

export type RowActionKind = "scoping" | "executing" | "landing" | "inactive";

export function actionKind(phase: PlanPhase, role: SessionRole): RowActionKind {
	if (phase === "scoping" && role === "scoping") return "scoping";
	if (phase === "executing" && role === "executing") return "executing";
	if (phase === "landing" && role === "landing") return "landing";
	return "inactive";
}

const DIRTY_TITLE = "Commit or discard worktree changes first";

export type RowActionsProps =
	| {
			kind: "scoping";
			planName: string;
			hasPlanMd: boolean;
			running: boolean;
			onExecute: () => void;
			onCancel: () => void;
	  }
	| {
			kind: "executing";
			planName: string;
			running: boolean;
			worktree: WorktreeStatus | null;
			onCancel: () => void;
			onFinishLanding: () => void;
			onBeginLanding: () => void;
	  }
	| {
			kind: "landing";
			planName: string;
			running: boolean;
			dirty: boolean;
			onCancel: () => void;
			onFinishLanding: () => void;
	  }
	| { kind: "inactive"; planName: string };

export function RowActions(props: RowActionsProps) {
	switch (props.kind) {
		case "scoping":
			return (
				<>
					<ActionButton
						className="cancel"
						label={`Cancel ${props.planName}`}
						disabled={props.running}
						onClick={props.onCancel}
					>
						✕
					</ActionButton>
					<ActionButton
						className="promote"
						label={`Send ${props.planName} to execution`}
						disabled={props.running || !props.hasPlanMd}
						onClick={props.onExecute}
					>
						&gt;
					</ActionButton>
				</>
			);
		case "executing": {
			const dirty = props.worktree?.dirty ?? false;
			const target = props.worktree?.target_branch ?? "";
			const needsLanding = props.worktree ? !props.worktree.ffable : false;
			const disabled = props.running || dirty;
			return (
				<>
					<ActionButton
						className="cancel"
						label={`Cancel ${props.planName} and delete branch`}
						disabled={props.running}
						onClick={props.onCancel}
					>
						✕
					</ActionButton>
					{needsLanding ? (
						<ActionButton
							className="promote"
							label={`Start landing ${props.planName} onto ${target}`}
							disabled={disabled}
							title={dirty ? DIRTY_TITLE : undefined}
							onClick={props.onBeginLanding}
						>
							&gt;
						</ActionButton>
					) : (
						<ActionButton
							className="promote"
							label={`Land ${props.planName} onto ${target}`}
							disabled={disabled}
							title={dirty ? DIRTY_TITLE : undefined}
							onClick={props.onFinishLanding}
						>
							✓
						</ActionButton>
					)}
				</>
			);
		}
		case "landing": {
			const disabled = props.running || props.dirty;
			return (
				<>
					<ActionButton
						className="cancel"
						label="Cancel landing and delete branch"
						disabled={props.running}
						onClick={props.onCancel}
					>
						✕
					</ActionButton>
					<ActionButton
						className="promote"
						label={`Finish landing ${props.planName}`}
						disabled={disabled}
						title={props.dirty ? DIRTY_TITLE : undefined}
						onClick={props.onFinishLanding}
					>
						✓
					</ActionButton>
				</>
			);
		}
		case "inactive":
			return null;
		default: {
			const _exhaustive: never = props;
			return _exhaustive;
		}
	}
}

function ActionButton({
	className,
	label,
	disabled,
	title,
	onClick,
	children,
}: {
	className: string;
	label: string;
	disabled: boolean;
	title?: string | undefined;
	onClick: () => void;
	children: string;
}) {
	return (
		<button
			className={`sbtn ${className}`}
			type="button"
			aria-label={label}
			disabled={disabled}
			title={title}
			onClick={(event) => {
				event.stopPropagation();
				onClick();
			}}
		>
			{children}
		</button>
	);
}
