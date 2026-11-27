import type { RepoDefaults } from "../bindings";
import type { Chats } from "../sessions/store";
import {
	OptionDropdown,
	OptionPlaceholder,
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
	const controlsDisabled = live === null || disabled;
	return (
		<>
			<div className="sect">
				<div className="slabel">MODEL</div>
				{model !== undefined ? (
					<OptionDropdown
						option={model}
						disabled={controlsDisabled}
						onChange={onChange}
					/>
				) : (
					<OptionPlaceholder label="Model unavailable" />
				)}
			</div>
			<div className="sect">
				<div className="slabel">EFFORT</div>
				{effort !== undefined ? (
					<OptionDropdown
						option={effort}
						disabled={controlsDisabled}
						onChange={onChange}
					/>
				) : (
					<OptionPlaceholder label="Effort unavailable" />
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
