import { useState } from "react";
import type { ConfigOptionValueView, ConfigOptionView } from "../types";

export function ModelSelector({
	options,
	disabled,
	onChange,
}: {
	options: ConfigOptionView[];
	disabled: boolean;
	onChange: (configId: string, value: string) => void;
}) {
	const model = options.find((option) => option.id === "model");
	const extras = options.filter(
		(option) => option.id !== "model" && option.id !== "mode",
	);
	return (
		<>
			{model !== undefined && (
				<OptionDropdown
					option={model}
					disabled={disabled}
					onChange={onChange}
				/>
			)}
			{extras.map((option) => (
				<OptionDropdown
					key={option.id}
					option={option}
					disabled={disabled}
					onChange={onChange}
				/>
			))}
		</>
	);
}

function OptionDropdown({
	option,
	disabled,
	onChange,
}: {
	option: ConfigOptionView;
	disabled: boolean;
	onChange: (configId: string, value: string) => void;
}) {
	const [open, setOpen] = useState(false);
	const selected: ConfigOptionValueView | undefined = option.options.find(
		(value) => value.value === option.currentValue,
	);
	const label =
		selected !== undefined ? `${option.name} / ${selected.name}` : option.name;
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
					{option.options.map((value) => (
						<button
							className="mop"
							key={value.value}
							type="button"
							data-value={value.value}
							onClick={() => {
								onChange(option.id, value.value);
								setOpen(false);
							}}
						>
							<span className="tick">
								{value.value === option.currentValue ? "✓" : ""}
							</span>
							{value.name}
						</button>
					))}
				</span>
			)}
		</span>
	);
}
