import { useId } from "react";
import { cx } from "./cx";

export interface ToggleProps {
  label: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  /** `type-meta`, `text-muted`, linked with `aria-describedby`. */
  description?: string;
  disabled?: boolean;
  /** Where the label goes relative to the switch. Column settings uses "start". */
  labelPosition?: "start" | "end";
  className?: string;
}

/** A switch (`role="switch"`) with its `<label>`: the label is clickable and names the switch. */
export function Toggle({
  label,
  checked,
  onCheckedChange,
  description,
  disabled = false,
  labelPosition = "end",
  className,
}: ToggleProps) {
  const id = useId();
  const descId = `${id}-description`;

  const control = (
    <button
      type="button"
      role="switch"
      id={id}
      aria-checked={checked}
      aria-disabled={disabled ? "true" : undefined}
      aria-describedby={description ? descId : undefined}
      onClick={() => {
        if (!disabled) onCheckedChange(!checked);
      }}
      className={cx(
        "inline-flex h-(--gr-space-5) w-(--gr-size-target-action) shrink-0 items-center rounded-pill border border-text-faint bg-surface-control cursor-pointer gr-ring gr-motion",
        "gr-hover:border-text-muted aria-checked:border-accent aria-checked:bg-accent gr-hover:aria-checked:border-accent",
        "aria-disabled:cursor-not-allowed aria-disabled:border-text-disabled",
      )}
    >
      <span
        aria-hidden="true"
        className={cx(
          "ml-(--gr-space-1) size-(--gr-space-3) rounded-pill gr-motion-transform",
          checked && "translate-x-(--gr-space-3)",
          disabled ? "bg-text-disabled" : checked ? "bg-text-on-accent" : "bg-text-faint",
        )}
      />
    </button>
  );

  const text = (
    <span className="flex min-h-(--gr-size-target-min) flex-col justify-center">
      <label htmlFor={id} className={cx("cursor-pointer [font:var(--gr-type-body)]", disabled ? "cursor-not-allowed text-text-disabled" : "text-text-main")}>
        {label}
      </label>
      {description ? (
        <span id={descId} className="text-text-muted [font:var(--gr-type-meta)]">
          {description}
        </span>
      ) : null}
    </span>
  );

  return (
    <div className={cx("inline-flex gap-(--gr-space-2)", description ? "items-start" : "items-center", className)}>
      {labelPosition === "start" ? text : control}
      {labelPosition === "start" ? control : text}
    </div>
  );
}
