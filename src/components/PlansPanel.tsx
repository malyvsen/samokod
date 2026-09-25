import type {
	PlanEntry,
	PlanPhase,
	SessionKey,
	SessionStatusView,
} from "../types";
import { sameSession } from "../types";

type DotClass = "running" | "input" | "approval" | "failed" | "done";

function dotClass(
	phase: PlanPhase,
	role: string,
	status: SessionStatusView,
): DotClass {
	if (status.failed) return "failed";
	if (status.approval) return "approval";
	if (status.working) return "running";
	if (role === "scoping" && phase !== "scoping") return "done";
	if (role === "executing" && phase !== "executing") return "done";
	return "input";
}

function sessionLabel(role: string): string {
	return role === "executing" ? "Execution" : "Scoping";
}

export function PlansPanel({
	plans,
	selected,
	onSelect,
	onNewPlan,
	onExecute,
	onAbandon,
	onCancel,
	onDone,
}: {
	plans: PlanEntry[];
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onNewPlan: () => void;
	onExecute: (session: SessionKey) => void;
	onAbandon: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onDone: (session: SessionKey) => void;
}) {
	return (
		<div className="plans">
			<button className="newplan" type="button" onClick={onNewPlan}>
				+ NEW PLAN
			</button>
			{plans.map((plan) => (
				<div className="plan-group" key={`${plan.phase}/${plan.name}`}>
					<div className="plan-name" data-full={plan.title}>
						<span className="ptitle">{plan.title}</span>
						<span className="phase-tag">{plan.phase.toUpperCase()}</span>
					</div>
					{plan.sessions.map((status) => {
						const key: SessionKey = { plan: plan.name, role: status.role };
						const isSelected = sameSession(selected, key);
						const dot = dotClass(plan.phase, status.role, status);
						return (
							<div
								className={`session${isSelected ? " selected" : ""}`}
								key={status.role}
							>
								<button
									className="srow"
									type="button"
									onClick={() => onSelect(key)}
									aria-label={`${plan.title} ${sessionLabel(status.role)}`}
								>
									<span className={`dot ${dot}`} aria-hidden="true" />
									<span className="slabel">{sessionLabel(status.role)}</span>
								</button>
								<RowActions
									phase={plan.phase}
									role={status.role}
									planName={plan.name}
									hasPlanMd={plan.has_plan_md}
									running={status.working}
									onExecute={() => onExecute(key)}
									onAbandon={() => onAbandon(key)}
									onCancel={() => onCancel(key)}
									onDone={() => onDone(key)}
								/>
							</div>
						);
					})}
				</div>
			))}
		</div>
	);
}

function RowActions({
	phase,
	role,
	planName,
	hasPlanMd,
	running,
	onExecute,
	onAbandon,
	onCancel,
	onDone,
}: {
	phase: PlanPhase;
	role: string;
	planName: string;
	hasPlanMd: boolean;
	running: boolean;
	onExecute: () => void;
	onAbandon: () => void;
	onCancel: () => void;
	onDone: () => void;
}) {
	if (phase === "scoping" && role === "scoping") {
		return (
			<>
				<ActionButton
					className="abandon"
					label={`Abandon ${planName}`}
					disabled={running}
					onClick={onAbandon}
				>
					✕
				</ActionButton>
				<ActionButton
					className="promote"
					label={`Send ${planName} to execution`}
					disabled={running || !hasPlanMd}
					onClick={onExecute}
				>
					&gt;
				</ActionButton>
			</>
		);
	}
	if (phase === "executing" && role === "executing") {
		return (
			<>
				<ActionButton
					className="abandon"
					label={`Cancel ${planName}`}
					disabled={running}
					onClick={onCancel}
				>
					✕
				</ActionButton>
				<ActionButton
					className="promote"
					label={`Mark ${planName} done`}
					disabled={running}
					onClick={onDone}
				>
					✓
				</ActionButton>
			</>
		);
	}
	return null;
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
