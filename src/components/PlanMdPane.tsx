import { MessageMarkdown } from "./MessageMarkdown";

export function PlanMdPane({ text }: { text: string | null }) {
	return (
		<div className="side planmd-pane">
			<div className="planmd">
				{text === null ? (
					<div className="empty">
						No plan yet. Keep scoping - the plan appears here once the agent
						writes it. Execute unlocks then.
					</div>
				) : (
					<MessageMarkdown text={text} />
				)}
			</div>
		</div>
	);
}
