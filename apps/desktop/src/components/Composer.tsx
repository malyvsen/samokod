import type { AgentStatus, ConfigOptionView } from "../types";
import { ModelSelector } from "./ModelSelector";

export function Composer({
	status,
	draft,
	configOptions,
	onDraft,
	onSend,
	onStop,
	onTypePulse,
}: {
	status: AgentStatus;
	draft: string;
	configOptions: ConfigOptionView[];
	onDraft: (text: string) => void;
	onSend: () => void;
	onStop: () => void;
	onTypePulse: () => void;
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
						onTypePulse();
					}}
					onKeyDown={(event) => {
						if (event.key === "Enter" && !busy && canSend) {
							onSend();
						}
					}}
				/>
				<ModelSelector options={configOptions} disabled={busy} />
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
