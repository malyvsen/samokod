import type { SessionKey, SessionStatusView } from "../../bindings";
import { formatContext, formatCost } from "../../spend";
import type { AttentionKind } from "./planDisplay";
import { roleLabel } from "./planDisplay";

export function SessionRow({
	planName,
	planTitle,
	status,
	openPath,
	trail,
	position,
	onSelect,
}: {
	planName: string;
	planTitle: string;
	status: SessionStatusView;
	openPath: AttentionKind | "sel";
	trail: "before" | "open" | "after";
	position: "single" | "last" | "middle";
	onSelect: (session: SessionKey) => void;
}) {
	const key: SessionKey = { plan: planName, role: status.role };
	return (
		<>
			<span className={gutterClass(position)} aria-hidden="true">
				<i className={`v${spineClass(openPath, trail, position)}`} />
				<i className={`h${nodeClass(openPath, trail)}`} />
				<i className={`nd${nodeClass(openPath, trail)}`} />
			</span>
			<div className={`sbody${trail === "open" ? " selected" : ""}`}>
				<button
					className="srow"
					type="button"
					onClick={() => onSelect(key)}
					aria-label={`${planTitle} ${roleLabel(status.role)}`}
				>
					<span className="slabel">{roleLabel(status.role)}</span>
				</button>
				<span className="sess-tip" aria-hidden="true">
					<span className="costline">
						<span>{formatCost(status.spend.cost ?? 0)}</span>
						<span>{formatContext(status.spend.ctx_pct ?? 0)}</span>
					</span>
				</span>
			</div>
		</>
	);
}

function gutterClass(position: "single" | "last" | "middle"): string {
	switch (position) {
		case "single":
			return "ngutter single";
		case "last":
			return "ngutter last";
		case "middle":
			return "ngutter";
	}
}

function spineClass(
	openPath: AttentionKind | "sel",
	trail: "before" | "open" | "after",
	position: "single" | "last" | "middle",
): string {
	switch (trail) {
		case "before":
			return ` c-${openPath}`;
		case "open":
			return position === "middle" ? ` u-${openPath}` : ` c-${openPath}`;
		case "after":
			return "";
	}
}

function nodeClass(
	openPath: AttentionKind | "sel",
	trail: "before" | "open" | "after",
): string {
	if (trail !== "open") {
		return "";
	}
	return openPath === "sel" ? " sel" : ` c-${openPath}`;
}
