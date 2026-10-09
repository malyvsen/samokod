import { MessageMarkdown } from "./MessageMarkdown";

export function PlanMdPane({ text }: { text: string | null }) {
	return (
		<div className="side planmd-pane">
			<div className="planmd">
				{text === null ? (
					<div className="empty">No plan yet.</div>
				) : (
					<MessageMarkdown text={text} />
				)}
			</div>
		</div>
	);
}
