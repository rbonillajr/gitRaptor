import { useRef } from "react";
import { cx } from "./cx";
import { useRovingFocus } from "./hooks/useRovingFocus";

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SegmentedControlProps<T extends string> {
  /** `aria-label` of the radio group. */
  label: string;
  options: ReadonlyArray<SegmentOption<T>>;
  value: T;
  onChange: (value: T) => void;
  disabled?: boolean;
  className?: string;
}

/** Mutually exclusive options (APG radio group): one Tab stop, arrows move and select, with wrap-around. */
export function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
  disabled = false,
  className,
}: SegmentedControlProps<T>) {
  const ref = useRef<HTMLDivElement>(null);
  const { onKeyDown } = useRovingFocus(ref, "[role='radio']", {
    orientation: "both",
    loop: true,
    skipDisabled: true,
    onMove: (index) => {
      const next = options[index];
      if (next) onChange(next.value);
    },
  });

  return (
    <div
      ref={ref}
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={cx(
        "inline-flex h-(--gr-size-target-action) items-center rounded-md bg-surface-control p-(--gr-space-1) shadow-(--gr-highlight)",
        className,
      )}
    >
      {options.map((option) => {
        const selected = option.value === value;
        const off = disabled || option.disabled === true;
        return (
          <button
            key={option.value}
            type="button"
            role="radio"
            aria-checked={selected}
            aria-disabled={off ? "true" : undefined}
            tabIndex={selected ? 0 : -1}
            onClick={() => {
              if (!off) onChange(option.value);
            }}
            className={cx(
              "inline-flex h-(--gr-size-target-min) items-center rounded-sm border border-transparent px-(--gr-space-3) text-text-muted cursor-pointer [font:var(--gr-type-secondary)] gr-ring-inset gr-motion",
              "gr-hover:bg-surface-hover gr-hover:text-text-main gr-pressed:bg-surface-hover",
              "aria-checked:border-accent aria-checked:bg-accent-glow aria-checked:text-text-main gr-hover:aria-checked:bg-accent-glow",
              "aria-disabled:cursor-not-allowed aria-disabled:text-text-disabled",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
