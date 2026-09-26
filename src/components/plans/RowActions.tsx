import type { PlanPhase, SessionRole } from "../../types";

export type RowActionKind = "scoping" | "executing" | "history";

export function actionKind(phase: PlanPhase, role: SessionRole): RowActionKind {
	if (phase === "scoping" && role === "scoping") return "scoping";
	if (phase === "executing" && role === "executing") return "executing";
	return "history";
}

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
		case "executing":
			return (
				<>
					<ActionButton
						className="abandon"
						label={`Cancel ${props.planName}`}
						disabled={props.running}
						onClick={props.onCancel}
					>
						✕
					</ActionButton>
					<ActionButton
						className="promote"
						label={`Mark ${props.planName} done`}
						disabled={props.running}
						onClick={props.onDone}
					>
						✓
					</ActionButton>
				</>
			);
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
	onClick,
	children,
}: {
	className: string;
	label: string;
	disabled: boolean;
	onClick: () => void;
	children: string;
}) {
	return (
		<button
			className={`sbtn ${className}`}
			type="button"
			aria-label={label}
			disabled={disabled}
			onClick={(event) => {
				event.stopPropagation();
				onClick();
			}}
		>
			{children}
		</button>
	);
}
