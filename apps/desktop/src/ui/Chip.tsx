import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "./cx";
import { Check, Close } from "./icons";
import { IconButton } from "./IconButton";

export interface ChipProps {
  label: string;
  /** `type-mono` for data (a hash, ↑4). */
  mono?: boolean;
  /** Only with `onToggle`. */
  selected?: boolean;
  onToggle?: (selected: boolean) => void;
  onRemove?: () => void;
  disabled?: boolean;
  className?: string;
}

const FRAME = "inline-flex items-center rounded-pill border bg-surface-control text-text-muted";
const IDLE = "border-border-strong";
const SELECTED = "border-accent bg-accent-glow text-text-main";

/**
 * A static tag, a filter toggle (`onToggle`) or a removable tag (`onRemove`). With `onRemove` the remove
 * button is a sibling of the chip's own button, never nested inside it.
 */
export function Chip({ label, mono = false, selected = false, onToggle, onRemove, disabled = false, className }: ChipProps) {
  const { t } = useTranslation();
  const font = mono ? "[font:var(--gr-type-mono)]" : "[font:var(--gr-type-caption)]";
  const on = onToggle !== undefined && selected;

  const toggle = () => {
    if (!disabled) onToggle?.(!selected);
  };
  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (onRemove && !disabled && (e.key === "Delete" || e.key === "Backspace")) {
      e.preventDefault();
      onRemove();
    }
  };

  const body = onToggle ? (
    <button
      type="button"
      aria-pressed={selected}
      aria-disabled={disabled ? "true" : undefined}
      onClick={toggle}
      onKeyDown={onKeyDown}
      className={cx(
        "inline-flex items-center gap-(--gr-space-1) rounded-pill cursor-pointer gr-ring gr-motion aria-disabled:cursor-not-allowed aria-disabled:text-text-disabled",
        onRemove ? "h-(--gr-size-target-min) bg-transparent text-inherit" : cx(FRAME, "h-(--gr-size-target-min) px-(--gr-space-2)", on ? SELECTED : IDLE, "gr-hover:bg-surface-hover gr-hover:text-text-main gr-hover:aria-pressed:bg-accent-glow"),
        font,
      )}
    >
      {on ? <Check size="sm" /> : null}
      {label}
    </button>
  ) : (
    <span className={cx("inline-flex items-center", font)}>{label}</span>
  );

  if (!onRemove) {
    return onToggle ? (
      <span className={cx("inline-flex", className)}>{body}</span>
    ) : (
      <span className={cx(FRAME, IDLE, "h-(--gr-space-5) px-(--gr-space-2)", font, className)}>{label}</span>
    );
  }

  return (
    <span className={cx(FRAME, on ? SELECTED : IDLE, "gap-(--gr-space-1) pl-(--gr-space-2)", disabled && "text-text-disabled", className)}>
      {body}
      <IconButton
        label={t("ui.chip.remove", { label })}
        icon={<Close size="sm" />}
        variant="ghost"
        size="sm"
        disabled={disabled}
        onClick={onRemove}
      />
    </span>
  );
}
