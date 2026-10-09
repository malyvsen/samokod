import type { Chats } from "../sessions/store";
import type { RepoDefaults } from "../types";
import {
	EffortPlaceholder,
	OptionDropdown,
	pendingOptions,
	splitOptions,
	toSelectorModel,
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
	const liveOptions = liveOptionsFrom(chats);
	const selectors =
		liveOptions === null
			? { kind: "pending" as const, defaults }
			: toSelectorModel(liveOptions, defaults);
	if (selectors.kind === "pending") {
		const pending = pendingOptions(selectors.defaults);
		return (
			<div className="plans settings">
				<div className="sect">
					<div className="slabel">MODEL</div>
					<OptionDropdown
						option={pending.model}
						disabled={true}
						onChange={onChange}
					/>
				</div>
				<div className="sect">
					<div className="slabel">EFFORT</div>
					<OptionDropdown
						option={pending.effort}
						disabled={true}
						onChange={onChange}
					/>
				</div>
			</div>
		);
	}
	const { model, effort, extras } = splitOptions(selectors.options);
	return (
		<div className="plans settings">
			{model !== undefined && (
				<div className="sect">
					<div className="slabel">MODEL</div>
					<OptionDropdown
						option={model}
						disabled={disabled}
						onChange={onChange}
					/>
				</div>
			)}
			<div className="sect">
				<div className="slabel">EFFORT</div>
				{effort !== undefined ? (
					<OptionDropdown
						option={effort}
						disabled={disabled}
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
					disabled={disabled}
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
