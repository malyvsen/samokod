import type {
	ConfigOptionValueView,
	ConfigOptionView,
	RepoDefaults,
} from "../bindings";
import { useDropdown } from "./dropdown";

export function pendingOptions(defaults: RepoDefaults): {
	model: ConfigOptionView;
	effort: ConfigOptionView;
} {
	return {
		model: frozenOption("model", "Model", defaults.model, "model"),
		effort: frozenOption("effort", "Effort", defaults.effort, "thought_level"),
	};
}

function frozenOption(
	id: string,
	name: string,
	value: string | null | undefined,
	category: string,
): ConfigOptionView {
	const current = value ?? null;
	return current === null
		? { id, name, currentValue: "", options: [], category }
		: {
				id,
				name,
				currentValue: current,
				options: [{ value: current, name: current }],
				category,
			};
}

export function splitOptions(options: ConfigOptionView[]): {
	model: ConfigOptionView | undefined;
	effort: ConfigOptionView | undefined;
} {
	const model = options.find(isModel);
	const effort = options.find(isEffort);
	return {
		model: model === undefined ? undefined : withSortedValues(model),
		effort,
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
	const { open, setOpen, rootRef } = useDropdown();
	const selected: ConfigOptionValueView | undefined = option.options.find(
		(value) => value.value === option.currentValue,
	);
	const label = selected !== undefined ? selected.name : option.name;
	const isFixed = option.options.length <= 1;
	return (
		<span className="dwrap" ref={rootRef}>
			<button
				className="dsel"
				type="button"
				disabled={disabled || isFixed}
				onClick={() => setOpen((value) => !value)}
				aria-label={option.name}
				aria-expanded={open}
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

export function OptionPlaceholder({ label }: { label: string }) {
	return (
		<span className="dwrap">
			<button className="dsel" type="button" disabled>
				<span className="dlabel">{label}</span>
			</button>
		</span>
	);
}
