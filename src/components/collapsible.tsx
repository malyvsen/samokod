import { useState } from "react";

export type CollapsibleKind = "agent" | "user" | "lead" | "queued";

export function isLongMessage(text: string): boolean {
	if (text.length > 600) return true;
	return text.split("\n").length > 10;
}

export function useCollapsed(
	kind: CollapsibleKind,
	text: string,
): [boolean, () => void] {
	const [collapsed, setCollapsed] = useState(
		() => isLongMessage(text) && kind !== "agent",
	);
	return [collapsed, () => setCollapsed((current) => !current)];
}

export function CollapseControl({
	collapsed,
	onToggle,
}: {
	collapsed: boolean;
	onToggle: () => void;
}) {
	const label = collapsed ? "Expand message" : "Collapse message";
	return (
		<button
			type="button"
			className="mctl"
			aria-label={label}
			title={label}
			onMouseDown={(event) => event.preventDefault()}
			onClick={(event) => {
				event.stopPropagation();
				onToggle();
			}}
		>
			{collapsed ? "+" : "-"}
		</button>
	);
}
