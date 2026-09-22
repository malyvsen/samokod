import { useState } from "react";
import type { ConfigOptionView, ConfigValueView } from "../types";

export function ModelSelector({
	options,
	disabled,
}: {
	options: ConfigOptionView[];
	disabled: boolean;
}) {
	const model = options.find((option) => option.id === "model");
	const extras = options.filter(
		(option) => option.id !== "model" && option.id !== "mode",
	);
	return (
		<>
			{model !== undefined && (
				<OptionDropdown option={model} disabled={disabled} />
			)}
			{extras.map((option) => (
				<OptionDropdown key={option.id} option={option} disabled={disabled} />
			))}
		</>
	);
}

function OptionDropdown({
	option,
	disabled,
}: {
	option: ConfigOptionView;
	disabled: boolean;
}) {
	const values: ConfigValueView[] = option.options;
	const [open, setOpen] = useState(false);
	const current = values.find((value) => value.value === option.current);
	const label =
		current !== undefined ? `${option.name} / ${current.name}` : option.name;
	return (
		<span className="mwrap">
			<button
				className="msel"
				type="button"
				disabled={disabled}
				onClick={() => setOpen((value) => !value)}
				aria-label={option.name}
			>
				<span className="mlabel">{label}</span>
				<span className="caret">▾</span>
			</button>
			{open && !disabled && (
				<span className="mpop">
					{values.map((value: ConfigValueView) => (
						<button
							className="mop"
							key={value.value}
							type="button"
							data-value={value.value}
							onClick={() => setOpen(false)}
						>
							<span className="tick">
								{value.value === option.current ? "✓" : ""}
							</span>
							{value.name}
						</button>
					))}
				</span>
			)}
		</span>
	);
}
