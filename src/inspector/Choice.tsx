// A value the Inspector offers from a menu, drawn as text with a small chevron: the Region
// inspector's Quantize, Dynamics and Gate Time, and the unit beside each Delay.
import { Icon } from "../ui/icons";
import { openMenuAt } from "../ui/Menu";
import { onPress } from "../keyboard/keyNav";
import { optionLabel } from "../tracks/regions/regionParams";

/** `value` among `options` ("*" when null: the selected regions differ). */
export function Choice({ value, options, onChange }: { value: number | null; options: [number, string][]; onChange: (v: number) => void }) {
  return (
    <button
      type="button"
      className="insp-popup"
      {...onPress((e) => {
        e.preventDefault();
        e.stopPropagation();
        openMenuAt(
          e.currentTarget,
          options.map(([v, label]) => ({ label, checked: v === value, onClick: () => onChange(v) })),
        );
      })}
    >
      {value === null ? "*" : optionLabel(options, value)}
      <Icon.chevron />
    </button>
  );
}

/** The Delay's unit choices: ticks (0) or ms (1). */
export const DELAY_UNITS: [number, string][] = [
  [0, "ticks"],
  [1, "ms"],
];
