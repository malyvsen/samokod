import type { AgentStatus } from "../types";

export function Composer({
	status,
	draft,
	onDraft,
	onSend,
	onStop,
	onEdit,
}: {
	status: AgentStatus;
	draft: string;
	onDraft: (text: string) => void;
	onSend: () => void;
	onStop: () => void;
	onEdit: () => void;
}) {
	const busy = status !== "idle";
	const canSend = draft.trim() !== "";
	const placeholder =
		status === "idle"
			? "Ask for a change…"
			: status === "working"
				? "Working - input disabled…"
				: "Paused - answer the approval…";
	return (
		<div className="composer">
			<div className="crow">
				<input
					className="cbox"
					disabled={busy}
					placeholder={placeholder}
					value={draft}
					onChange={(event) => {
						onDraft(event.target.value);
						onEdit();
					}}
					onKeyDown={(event) => {
						if (event.key === "Enter" && !busy && canSend) {
							onSend();
						}
					}}
				/>
				{busy ? (
					<button className="send stop" type="button" onClick={onStop}>
						STOP
					</button>
				) : (
					<button
						className="send"
						type="button"
						disabled={!canSend}
						onClick={onSend}
					>
						SEND
					</button>
				)}
			</div>
		</div>
	);
}
