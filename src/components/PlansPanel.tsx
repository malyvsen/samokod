import type { PlanEntry, SessionKey } from "../types";
import { SessionRow } from "./plans/SessionRow";

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
					{plan.sessions.map((status) => (
						<SessionRow
							key={status.role}
							planName={plan.name}
							planTitle={plan.title}
							phase={plan.phase}
							hasPlanMd={plan.has_plan_md}
							status={status}
							selected={selected}
							onSelect={onSelect}
							onExecute={onExecute}
							onAbandon={onAbandon}
							onCancel={onCancel}
							onDone={onDone}
						/>
					))}
				</div>
			))}
		</div>
	);
}
