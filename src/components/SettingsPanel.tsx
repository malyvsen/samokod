import type { RepoDefaults } from "../bindings";
import type { Chats } from "../sessions/store";
import {
	EffortPlaceholder,
	ModelPlaceholder,
	OptionDropdown,
	pendingOptions,
	splitOptions,
} from "./selectors";

export function SettingsPanel({
	chats,
	defaults,
	disabled,
	onChange,
}: {
	chats: Chats;
	defaults: RepoDefaults;
	disabled: boolean;
	onChange: (configId: string, value: string) => void;
}) {
	const live = liveOptionsFrom(chats);
	const { model, effort } =
		live === null ? pendingOptions(defaults) : splitOptions(live);
	const frozen = live === null;
	return (
		<>
			<div className="sect">
				<div className="slabel">MODEL</div>
				{model !== undefined ? (
					<OptionDropdown
						option={model}
						disabled={frozen || disabled}
						onChange={onChange}
					/>
				) : (
					<ModelPlaceholder />
				)}
			</div>
			<div className="sect">
				<div className="slabel">EFFORT</div>
				{effort !== undefined ? (
					<OptionDropdown
						option={effort}
						disabled={frozen || disabled}
						onChange={onChange}
					/>
				) : (
					<EffortPlaceholder />
				)}
			</div>
		</>
	);
}

function liveOptionsFrom(chats: Chats) {
	for (const chat of Object.values(chats)) {
		if (chat.configOptions.length > 0) {
			return chat.configOptions;
		}
	}
	return null;
}
