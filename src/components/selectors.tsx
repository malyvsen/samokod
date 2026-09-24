import { useState } from "react";
import type { ConfigOptionValueView, ConfigOptionView } from "../types";

export function splitOptions(options: ConfigOptionView[]): {
	model: ConfigOptionView | undefined;
	effort: ConfigOptionView | undefined;
	extras: ConfigOptionView[];
} {
	const model = options.find(isModel);
	const effort = options.find(isEffort);
	const extras = options.filter(
		(option) => !isModel(option) && !isEffort(option) && !isMode(option),
	);
	return {
		model: model === undefined ? undefined : withSortedValues(model),
		effort,
		extras,
	};
}

function categoryOf(option: ConfigOptionView): string {
	return option.category ?? option.id;
}

function isModel(option: ConfigOptionView): boolean {
	return categoryOf(option) === "model";
}

function isEffort(option: ConfigOptionView): boolean {
	return categoryOf(option) === "thought_level" || option.id === "effort";
}

function isMode(option: ConfigOptionView): boolean {
	return categoryOf(option) === "mode";
}

function withSortedValues(option: ConfigOptionView): ConfigOptionView {
	return {
		...option,
		options: [...option.options].sort((a, b) =>
			a.name.localeCompare(b.name, undefined, { sensitivity: "base" }),
		),
	};
}

export function OptionDropdown({
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
	const label = selected !== undefined ? selected.name : option.name;
	const isFixed = option.options.length <= 1;
	return (
		<span className="dwrap">
			<button
				className="dsel"
				type="button"
				disabled={disabled || isFixed}
				onClick={() => setOpen((value) => !value)}
				aria-label={option.name}
			>
				<span className="dlabel">{label}</span>
				<span className="arrow">▾</span>
			</button>
			{open && !disabled && !isFixed && (
				<span className="dpop">
					{option.options.map((value) => (
						<button
							className="dopt"
							key={value.value}
							type="button"
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

export function EffortPlaceholder() {
	return (
		<span className="dwrap">
			<button className="dsel" type="button" disabled>
				<span className="dlabel">Effort unavailable</span>
			</button>
		</span>
	);
}
