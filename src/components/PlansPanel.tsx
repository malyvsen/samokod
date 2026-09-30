import { Fragment } from "react";
import type { PlanEntry, PlanPhase, SessionKey } from "../types";
import {
	attentionFor,
	attentionTitle,
	headerKey,
	PHASES,
	roleLabel,
} from "./plans/planDisplay";
import { SessionRow } from "./plans/SessionRow";

export function PlansPanel({
	plans,
	selected,
	onSelect,
	onNewPlan,
	onExecute,
	onCancel,
	onDone,
	onBeginMerge,
}: {
	plans: PlanEntry[];
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onNewPlan: () => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onDone: (session: SessionKey) => void;
	onBeginMerge: (session: SessionKey) => void;
}) {
	const byPhase = groupByPhase(plans);
	return (
		<div className="plans">
			<button className="newplan" type="button" onClick={onNewPlan}>
				+ NEW PLAN
			</button>
			{PHASES.map((phase) => (
				<Fragment key={phase}>
					<div className="sect-head">{phase.toUpperCase()}</div>
					{(byPhase.get(phase) ?? []).map((plan) => (
						<PlanGroup
							key={plan.name}
							plan={plan}
							phase={phase}
							expanded={selected?.plan === plan.name}
							selected={selected}
							onSelect={onSelect}
							onExecute={onExecute}
							onCancel={onCancel}
							onDone={onDone}
							onBeginMerge={onBeginMerge}
						/>
					))}
				</Fragment>
			))}
		</div>
	);
}

function PlanGroup({
	plan,
	phase,
	expanded,
	selected,
	onSelect,
	onExecute,
	onCancel,
	onDone,
	onBeginMerge,
}: {
	plan: PlanEntry;
	phase: PlanPhase;
	expanded: boolean;
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onDone: (session: SessionKey) => void;
	onBeginMerge: (session: SessionKey) => void;
}) {
	const target = headerKey(plan);
	const attention = attentionFor(plan);
	return (
		<div className="plan-group">
			<button
				className="plan-name"
				type="button"
				onClick={() => onSelect(target)}
				aria-label={`${plan.title}, ${phase}, opens ${roleLabel(target.role)}`}
				data-full={plan.title}
			>
				{attention === null ? null : (
					<span
						className={`adot ${attention}`}
						title={attentionTitle(attention)}
						aria-hidden="true"
					/>
				)}
				<span className="ptitle">{plan.title}</span>
			</button>
			{expanded
				? plan.sessions.map((status) => (
						<SessionRow
							key={status.role}
							planName={plan.name}
							planTitle={plan.title}
							phase={plan.phase}
							hasPlanMd={plan.has_plan_md}
							worktree={plan.worktree}
							status={status}
							selected={selected}
							onSelect={onSelect}
							onExecute={onExecute}
							onCancel={onCancel}
							onDone={onDone}
							onBeginMerge={onBeginMerge}
						/>
					))
				: null}
		</div>
	);
}

function groupByPhase(plans: PlanEntry[]): Map<PlanPhase, PlanEntry[]> {
	const groups = new Map<PlanPhase, PlanEntry[]>();
	for (const plan of plans) {
		const list = groups.get(plan.phase);
		if (list === undefined) {
			groups.set(plan.phase, [plan]);
		} else {
			list.push(plan);
		}
	}
	return groups;
}
