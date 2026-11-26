import { Fragment } from "react";
import type { Phase, PlanEntry, SessionKey } from "../bindings";
import { PlanMenu } from "./plans/PlanMenu";
import {
	type AttentionKind,
	attentionFor,
	attentionTitle,
	barTip,
	headerKey,
	headerMeta,
	headerProgress,
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
	onSetMode,
	onSetEvergreen,
}: {
	plans: PlanEntry[];
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onNewPlan: () => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onSetMode: (plan: string, manual: boolean) => void;
	onSetEvergreen: (plan: string, evergreen: boolean) => void;
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
							onSetMode={onSetMode}
							onSetEvergreen={onSetEvergreen}
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
	onSetMode,
	onSetEvergreen,
}: {
	plan: PlanEntry;
	selected: SessionKey | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onSetMode: (plan: string, manual: boolean) => void;
	onSetEvergreen: (plan: string, evergreen: boolean) => void;
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
	const marker = markerKindFor(attention, expanded);
	const progress = headerProgress(plan);
	const meta = progress === null ? null : headerMeta(progress, attention);
	const tip = progress === null ? null : barTip(progress);
	return (
		<div
			className={`plan-group tree${expanded ? " sel" : ""}${marker === null ? " quiet" : ""}`}
		>
			<span className="mgutter">
				<Spine expanded={expanded} openIdx={openIdx} openPath={openPath} />
				{marker === null ? null : (
					<span
						className={`mk mk-${marker}`}
						title={attention === null ? undefined : attentionTitle(attention)}
						aria-hidden="true"
					/>
				)}
			</span>
			<PlanHeader
				plan={plan}
				target={target}
				attention={attention}
				eta={meta?.eta ?? null}
				onSelect={onSelect}
				onExecute={onExecute}
				onCancel={onCancel}
				onSetMode={onSetMode}
				onSetEvergreen={onSetEvergreen}
			/>
			{meta === null ? null : (
				<>
					<span className="bgutter">
						<Spine expanded={expanded} openIdx={openIdx} openPath={openPath} />
					</span>
					<div className="hbar-cell">
						<div
							className={`hbar-in${meta.barClass ? ` ${meta.barClass}` : ""}`}
						>
							<i style={{ width: `${meta.pct}%` }} />
						</div>
						{tip === null ? null : <div className="hbar-tip">{tip}</div>}
					</div>
				</>
			)}
			{expanded
				? plan.sessions.map((status, index) => (
						<SessionRow
							key={status.role}
							planName={plan.name}
							planTitle={plan.title}
							status={status}
							openPath={openPath}
							trail={trailFor(index, openIdx)}
							position={positionFor(index, plan.sessions.length)}
							onSelect={onSelect}
						/>
					))
				: null}
		</div>
	);
}

function PlanHeader({
	plan,
	target,
	attention,
	eta,
	onSelect,
	onExecute,
	onCancel,
	onSetMode,
	onSetEvergreen,
}: {
	plan: PlanEntry;
	target: SessionKey;
	attention: AttentionKind | null;
	eta: string | null;
	onSelect: (session: SessionKey) => void;
	onExecute: (session: SessionKey) => void;
	onCancel: (session: SessionKey) => void;
	onSetMode: (plan: string, manual: boolean) => void;
	onSetEvergreen: (plan: string, evergreen: boolean) => void;
}) {
	return (
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
				{eta == null ? null : <span className="pmeta">{eta}</span>}
			</button>
			<PlanMenu
				plan={plan}
				target={target}
				onExecute={onExecute}
				onCancel={onCancel}
				onSetMode={onSetMode}
				onSetEvergreen={onSetEvergreen}
			/>
		</div>
	);
}

function Spine({
	expanded,
	openIdx,
	openPath,
}: {
	expanded: boolean;
	openIdx: number;
	openPath: AttentionKind | "sel";
}) {
	if (!expanded) {
		return null;
	}
	return (
		<i
			className={`v${openIdx >= 0 ? ` c-${openPath}` : ""}`}
			aria-hidden="true"
		/>
	);
}

function markerKindFor(
	attention: AttentionKind | null,
	expanded: boolean,
): AttentionKind | "sel" | null {
	if (attention !== null) {
		return attention;
	}
	return expanded ? "sel" : null;
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

function groupByPhase(plans: PlanEntry[]): Map<Phase, PlanEntry[]> {
	const groups = new Map<Phase, PlanEntry[]>();
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
