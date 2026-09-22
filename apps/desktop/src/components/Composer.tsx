import type { ConfigOptionView } from "../types";
import { ModelSelector } from "./ModelSelector";

export function Composer({
	draft,
	configOptions,
	wired,
	onDraft,
	onTypePulse,
}: {
	draft: string;
	configOptions: ConfigOptionView[];
	wired: boolean;
	onDraft: (text: string) => void;
	onTypePulse: () => void;
}) {
	const disabled = !wired;
	return (
		<div className="composer">
			<div className="crow">
				<input
					className="cbox"
					disabled={disabled}
					placeholder="Ask for a change…"
					value={draft}
					onChange={(event) => {
						onDraft(event.target.value);
						onTypePulse();
					}}
				/>
				<ModelSelector options={configOptions} disabled={disabled} />
				<button className="send" type="button" disabled>
					SEND
				</button>
			</div>
		</div>
	);
}
