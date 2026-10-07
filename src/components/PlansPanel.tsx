import { Fragment } from "react";
import type { PlanEntry, PlanPhase, SessionKey } from "../types";
import {
	type AttentionKind,
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
	onSetMode,
}: {
	plans: PlanEntry[];
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onNewPlan: () => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onFinishLanding: (session: SessionKey) => void;
	onBeginLanding: (session: SessionKey) => void;
	onSetMode: (plan: string, manual: boolean) => void;
}) {
	const byPhase = groupByPhase(plans);
	return (
		<div className="plans">
			{PHASES.map((phase) => (
				<Fragment key={phase}>
					<div className="sect-head">
						<span>{phase.toUpperCase()}</span>
						{phase === "scoping" ? (
							<button
								className="sbtn new"
								type="button"
								aria-label="New plan"
								onClick={onNewPlan}
							>
								+
							</button>
						) : null}
					</div>
					{(byPhase.get(phase) ?? []).map((plan) => (
						<PlanGroup
							key={plan.name}
							plan={plan}
							selected={selected}
							onSelect={onSelect}
							onExecute={onExecute}
							onCancel={onCancel}
							onFinishLanding={onFinishLanding}
							onBeginLanding={onBeginLanding}
							onSetMode={onSetMode}
						/>
					))}
				</Fragment>
			))}
		</div>
	);
}

function PlanGroup({
	plan,
	selected,
	onSelect,
	onExecute,
	onCancel,
	onFinishLanding,
	onBeginLanding,
	onSetMode,
}: {
	plan: PlanEntry;
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onFinishLanding: (session: SessionKey) => void;
	onBeginLanding: (session: SessionKey) => void;
	onSetMode: (plan: string, manual: boolean) => void;
}) {
	const target = headerKey(plan);
	const attention = attentionFor(plan);
	const expanded = selected !== null && selected.plan === plan.name;
	const openRole = expanded ? selected.role : null;
	const openPath = openRole === null ? "sel" : openPathFor(plan, openRole);
	const openIdx =
		openRole === null
			? -1
			: plan.sessions.findIndex((status) => status.role === openRole);
	return (
		<div className={`plan-group tree${expanded ? " sel" : ""}`}>
			<span className="mgutter">
				{expanded ? (
					<i
						className={`v${openIdx >= 0 ? ` c-${openPath}` : ""}`}
						aria-hidden="true"
					/>
				) : null}
				<span
					className={markerClassFor(attention, expanded)}
					title={attention === null ? undefined : attentionTitle(attention)}
					aria-hidden="true"
				/>
			</span>
			<div className="phead">
				<button
					className="plan-name"
					type="button"
					onClick={() => onSelect(target)}
					aria-label={`${plan.title}, ${plan.phase}, opens ${roleLabel(target.role)}`}
					data-full={plan.title}
				>
					<span
						className={`ptitle${attention === null ? "" : ` sheen-${attention}`}`}
					>
						{plan.title}
					</span>
				</button>
				{plan.phase === "completed" || plan.phase === "cancelled" ? null : (
					<span className="hact">
						<button
							className="sbtn cancel"
							type="button"
							aria-label={`Cancel ${plan.title}`}
							onClick={(event) => {
								event.stopPropagation();
								onCancel(target);
							}}
						>
							✕
						</button>
						<button
							className={`sbtn mode${plan.manual ? " manual" : ""}`}
							type="button"
							aria-label={plan.manual ? "Switch to auto" : "Switch to manual"}
							onClick={(event) => {
								event.stopPropagation();
								onSetMode(plan.name, !plan.manual);
							}}
						>
							{plan.manual ? "M" : "A"}
						</button>
					</span>
				)}
			</div>
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
							trail={trailFor(index, openIdx)}
							position={positionFor(index, plan.sessions.length)}
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

function markerClassFor(
	attention: AttentionKind | null,
	expanded: boolean,
): string {
	if (attention !== null) {
		return `mk mk-${attention}`;
	}
	return expanded ? "mk mk-sel" : "mk mk-none";
}

function trailFor(index: number, openIdx: number): "before" | "open" | "after" {
	if (index === openIdx) {
		return "open";
	}
	if (openIdx >= 0 && index < openIdx) {
		return "before";
	}
	return "after";
}

function positionFor(
	index: number,
	total: number,
): "single" | "last" | "middle" {
	if (total === 1) {
		return "single";
	}
	if (index === total - 1) {
		return "last";
	}
	return "middle";
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
