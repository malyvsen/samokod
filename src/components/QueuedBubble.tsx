import type { QueuedMessage } from "../sessions/queue";

export function QueuedBubbleList({ items }: { items: QueuedMessage[] }) {
	if (items.length === 0) return null;
	return (
		<>
			{items.map((item) => (
				<QueuedBubble key={item.id} text={item.text} />
			))}
		</>
	);
}

function QueuedBubble({ text }: { text: string }) {
	return (
		<div className="queued">
			<div className="who-line">QUEUED</div>
			{text}
		</div>
	);
}
