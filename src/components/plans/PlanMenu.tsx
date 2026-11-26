import type { PlanEntry, SessionKey } from "../../bindings";
import { formatCost } from "../../spend";
import { useDropdown } from "../dropdown";
import { planTotalCost, roleLabel } from "./planDisplay";

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
	const everOn = plan.evergreen;
	const total = planTotalCost(plan);
	const hasOptions = plan.phase !== "completed" && plan.phase !== "cancelled";
	if (!hasOptions && total === null) {
		return null;
	}
	const breakdown =
		total === null ? null : (
			<div className="mcost">
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
			</div>
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
					{hasOptions ? (
						plan.phase === "scoping" ? (
							<>
								<button
									className={`dopt${everOn ? " on" : ""}`}
									type="button"
									role="menuitemcheckbox"
									aria-checked={everOn}
									onClick={() => onSetEvergreen(plan.name, !everOn)}
									title={
										everOn
											? "Will clean after rebasing. Click to rebase then finish directly."
											: "Rebasing then finishing directly. Click to clean after rebasing."
									}
								>
									<span className="mark" aria-hidden="true">
										{everOn ? "[x]" : "[ ]"}
									</span>
									Evergreen
								</button>
								<div className="dsep" />
								<button
									className="dopt"
									type="button"
									role="menuitem"
									disabled={scopingWorking || !plan.has_plan_md}
									onClick={() => {
										setOpen(false);
										onExecute(target);
									}}
									title="Move to executing and start the agent"
								>
									Execute
								</button>
								{breakdown === null ? null : (
									<>
										<div className="dsep" />
										{breakdown}
										<div className="dsep" />
									</>
								)}
							</>
						) : (
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
								{plan.phase === "executing" ? (
									<button
										className={`dopt${everOn ? " on" : ""}`}
										type="button"
										role="menuitemcheckbox"
										aria-checked={everOn}
										onClick={() => onSetEvergreen(plan.name, !everOn)}
										title={
											everOn
												? "Will clean after rebasing. Click to rebase then finish directly."
												: "Rebasing then finishing directly. Click to clean after rebasing."
										}
									>
										<span className="mark" aria-hidden="true">
											{everOn ? "[x]" : "[ ]"}
										</span>
										Evergreen
									</button>
								) : null}
								<div className="dsep" />
								{breakdown === null ? null : (
									<>
										{breakdown}
										<div className="dsep" />
									</>
								)}
							</>
						)
					) : (
						breakdown
					)}
					{hasOptions ? (
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
					) : null}
				</span>
			) : null}
		</span>
	);
}
