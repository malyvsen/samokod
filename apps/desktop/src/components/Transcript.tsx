import ReactMarkdown from "react-markdown";
import type { ToolLineView, TranscriptItem } from "../types";

export function Transcript({
	items,
	onRetry,
}: {
	items: TranscriptItem[];
	onRetry: () => void;
}) {
	return (
		<div className="tcol">
			{items.map((item) => {
				if (item.kind === "user") {
					return (
						<div className="msg user" key={item.id}>
							<div className="who">YOU</div>
							{item.text}
						</div>
					);
				}
				if (item.kind === "agent") {
					return (
						<div className="msg agent" key={item.id}>
							<div className="who">AGENT</div>
							<ReactMarkdown>{item.text}</ReactMarkdown>
						</div>
					);
				}
				if (item.kind === "tool") {
					return <ToolRow key={item.id} line={item.line} />;
				}
				return (
					<div className="errbar" key={item.id}>
						<div className="erow">
							<span data-full={item.raw}>✕ {item.raw}</span>
							<button className="tbtn" type="button" onClick={onRetry}>
								retry
							</button>
						</div>
						<div className="ehint">retry the turn</div>
					</div>
				);
			})}
		</div>
	);
}

function ToolRow({ line }: { line: ToolLineView }) {
	return (
		<div className="tool" data-full={line.text}>
			{line.text}
		</div>
	);
}
