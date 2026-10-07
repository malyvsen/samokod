import { Fragment } from "react";
import type { PlanEntry, PlanPhase, SessionKey } from "../types";
import {
	attentionFor,
	attentionTitle,
	headerKey,
	openPathFor,
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
	onFinishLanding,
	onBeginLanding,
}: {
	plans: PlanEntry[];
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onNewPlan: () => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onFinishLanding: (session: SessionKey) => void;
	onBeginLanding: (session: SessionKey) => void;
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
							onFinishLanding={onFinishLanding}
							onBeginLanding={onBeginLanding}
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
	onFinishLanding,
	onBeginLanding,
}: {
	plan: PlanEntry;
	phase: PlanPhase;
	expanded: boolean;
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onFinishLanding: (session: SessionKey) => void;
	onBeginLanding: (session: SessionKey) => void;
}) {
	const target = headerKey(plan);
	const attention = attentionFor(plan);
	const openRole = selected?.plan === plan.name ? selected.role : null;
	const openPath = openRole === null ? "neutral" : openPathFor(plan, openRole);
	const openIdx =
		openRole === null
			? -1
			: plan.sessions.findIndex((status) => status.role === openRole);
	const markerClass =
		attention === null
			? expanded
				? "mk mk-sel"
				: "mk mk-none"
			: `mk mk-${attention}`;
	const pathKey = openPath === "neutral" ? "sel" : openPath;
	return (
		<div className="plan-group tree">
			<span className="mgutter">
				{expanded ? (
					<i
						className={`v${openIdx >= 0 ? ` c-${pathKey}` : ""}`}
						aria-hidden="true"
					/>
				) : null}
				<span
					className={markerClass}
					title={attention === null ? undefined : attentionTitle(attention)}
					aria-hidden="true"
				/>
			</span>
			<button
				className="plan-name"
				type="button"
				onClick={() => onSelect(target)}
				aria-label={`${plan.title}, ${phase}, opens ${roleLabel(target.role)}`}
				data-full={plan.title}
			>
				<span
					className={`ptitle${attention === null ? "" : ` gleam-${attention}`}`}
				>
					{plan.title}
				</span>
			</button>
			{expanded
				? plan.sessions.map((status, index) => (
						<SessionRow
							key={status.role}
							planName={plan.name}
							planTitle={plan.title}
							phase={plan.phase}
							hasPlanMd={plan.has_plan_md}
							worktree={plan.worktree}
							status={status}
							openPath={openPath}
							isOpen={index === openIdx}
							isBeforeOpen={openIdx >= 0 && index < openIdx}
							isLast={index === plan.sessions.length - 1}
							isSingle={plan.sessions.length === 1}
							onSelect={onSelect}
							onExecute={onExecute}
							onCancel={onCancel}
							onFinishLanding={onFinishLanding}
							onBeginLanding={onBeginLanding}
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
