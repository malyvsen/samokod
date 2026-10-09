import { MessageMarkdown } from "./MessageMarkdown";

export function PlanMdPane({ text }: { text: string | null }) {
	if (text === null) {
		return (
			<div className="side planmd-pane">
				<div className="planmd">
					<div className="empty">No plan yet.</div>
				</div>
			</div>
		);
	}
	return (
		<div className="side planmd-pane">
			<div className="planmd">
				<MessageMarkdown text={text} />
			</div>
		</div>
	);
}
