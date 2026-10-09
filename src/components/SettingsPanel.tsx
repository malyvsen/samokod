import type { Chats } from "../sessions/store";
import type { RepoDefaults } from "../types";
import {
	EffortPlaceholder,
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
	const { model, effort, extras } =
		live === null
			? { ...pendingOptions(defaults), extras: [] }
			: splitOptions(live);
	const frozen = live === null;
	return (
		<div className="plans settings">
			{model !== undefined && (
				<div className="sect">
					<div className="slabel">MODEL</div>
					<OptionDropdown
						option={model}
						disabled={frozen || disabled}
						onChange={onChange}
					/>
				</div>
			)}
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
			{extras.map((option) => (
				<OptionDropdown
					key={option.id}
					option={option}
					disabled={frozen || disabled}
					onChange={onChange}
				/>
			))}
		</div>
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
