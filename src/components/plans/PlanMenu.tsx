import type { PlanEntry, SessionKey } from "../../bindings";
import { formatCost, planTotalCost } from "../../spend";
import { useDropdown } from "../dropdown";
import { roleLabel } from "./planDisplay";

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
	const { open, setOpen, rootRef } = useDropdown();

	const scopingWorking =
		plan.sessions.find((status) => status.role === "scoping")?.working ?? false;
	const paused = plan.manual;
	const finished = plan.phase === "completed" || plan.phase === "cancelled";
	const isScoping = plan.phase === "scoping";
	const showEvergreen =
		isScoping ||
		plan.phase === "executing" ||
		plan.phase === "queued" ||
		plan.phase === "rebasing";
	const executeBlocked = scopingWorking || !plan.has_plan_md;
	const total = planTotalCost(plan);
	const toggleEvergreen = () => onSetEvergreen(plan.name, !plan.evergreen);
	const costRows =
		total === null ? (
			finished ? (
				<div className="dopt static">
					<span>No cost recorded</span>
				</div>
			) : null
		) : (
			<CostBreakdown plan={plan} total={total} />
		);

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
					{!finished && isScoping ? (
						<>
							<EvergreenOption on={plan.evergreen} onToggle={toggleEvergreen} />
							<div className="dsep" />
							<button
								className="dopt"
								type="button"
								role="menuitem"
								disabled={executeBlocked}
								onClick={() => {
									setOpen(false);
									onExecute(target);
								}}
								title={
									executeBlocked
										? "Plan must be written before execution"
										: "Move to executing and start the agent"
								}
							>
								Execute
							</button>
							{costRows === null ? null : (
								<>
									<div className="dsep" />
									{costRows}
									<div className="dsep" />
								</>
							)}
						</>
					) : null}
					{!finished && !isScoping ? (
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
							{showEvergreen ? (
								<EvergreenOption
									on={plan.evergreen}
									onToggle={toggleEvergreen}
								/>
							) : null}
							<div className="dsep" />
							{costRows === null ? null : (
								<>
									{costRows}
									<div className="dsep" />
								</>
							)}
						</>
					) : null}
					{finished ? costRows : null}
					{finished ? null : (
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
							Cancel
						</button>
					)}
				</span>
			) : null}
		</span>
	);
}

function EvergreenOption({
	on,
	onToggle,
}: {
	on: boolean;
	onToggle: () => void;
}) {
	return (
		<button
			className={`dopt${on ? " on" : ""}`}
			type="button"
			role="menuitemcheckbox"
			aria-checked={on}
			onClick={onToggle}
			title={
				on
					? "Will clean after rebasing. Click to rebase then finish directly."
					: "Rebasing then finishing directly. Click to clean after rebasing."
			}
		>
			<span className="mark" aria-hidden="true">
				{on ? "[x]" : "[ ]"}
			</span>
			Evergreen
		</button>
	);
}

function CostBreakdown({ plan, total }: { plan: PlanEntry; total: number }) {
	return (
		<>
			<div className="dopt static">
				<span>Total cost</span>
				<span className="mamt">{formatCost(total)}</span>
			</div>
			{plan.sessions.map((status) =>
				status.spend.cost == null ? null : (
					<div className="dopt static" key={status.role}>
						<span>{roleLabel(status.role)}</span>
						<span className="mamt">{formatCost(status.spend.cost)}</span>
					</div>
				),
			)}
		</>
	);
}
