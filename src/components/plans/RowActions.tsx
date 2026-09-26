import type { PlanPhase, SessionRole, WorktreeStatus } from "../../types";

export type RowActionKind = "scoping" | "executing" | "merging" | "history";

export function actionKind(phase: PlanPhase, role: SessionRole): RowActionKind {
	if (phase === "scoping" && role === "scoping") return "scoping";
	if (phase === "executing" && role === "executing") return "executing";
	if (phase === "merging" && role === "merging") return "merging";
	return "history";
}

const DIRTY_TITLE = "Commit or discard worktree changes first";

export type RowActionsProps =
	| {
			kind: "scoping";
			planName: string;
			hasPlanMd: boolean;
			running: boolean;
			onExecute: () => void;
			onAbandon: () => void;
	  }
	| {
			kind: "executing";
			planName: string;
			running: boolean;
			worktree: WorktreeStatus | null;
			onCancel: () => void;
			onDone: () => void;
			onBeginMerge: () => void;
	  }
	| {
			kind: "merging";
			planName: string;
			running: boolean;
			dirty: boolean;
			onCancel: () => void;
			onDone: () => void;
	  }
	| { kind: "history"; planName: string };

export function RowActions(props: RowActionsProps) {
	switch (props.kind) {
		case "scoping":
			return (
				<>
					<ActionButton
						className="abandon"
						label={`Abandon ${props.planName}`}
						disabled={props.running}
						onClick={props.onAbandon}
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
			const needsMerge = props.worktree ? !props.worktree.ffable : false;
			const disabled = props.running || dirty;
			return (
				<>
					<ActionButton
						className="abandon"
						label={`Cancel ${props.planName} and delete branch`}
						disabled={props.running}
						onClick={props.onCancel}
					>
						✕
					</ActionButton>
					{needsMerge ? (
						<ActionButton
							className="promote"
							label={`Rebase ${props.planName} onto latest main`}
							disabled={disabled}
							title={dirty ? DIRTY_TITLE : undefined}
							onClick={props.onBeginMerge}
						>
							&gt;
						</ActionButton>
					) : (
						<ActionButton
							className="promote"
							label={`Merge ${props.planName} to main`}
							disabled={disabled}
							title={dirty ? DIRTY_TITLE : undefined}
							onClick={props.onDone}
						>
							✓
						</ActionButton>
					)}
				</>
			);
		}
		case "merging": {
			const disabled = props.running || props.dirty;
			return (
				<>
					<ActionButton
						className="abandon"
						label="Cancel merge and delete branch"
						disabled={props.running}
						onClick={props.onCancel}
					>
						✕
					</ActionButton>
					<ActionButton
						className="promote"
						label={`Finish ${props.planName} merge`}
						disabled={disabled}
						title={props.dirty ? DIRTY_TITLE : undefined}
						onClick={props.onDone}
					>
						✓
					</ActionButton>
				</>
			);
		}
		case "history":
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
