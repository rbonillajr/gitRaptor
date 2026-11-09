import type { ComponentPropsWithRef, MouseEvent, ReactNode } from "react";
import { cx } from "./cx";

export interface IconButtonProps extends Omit<ComponentPropsWithRef<"button">, "disabled" | "aria-label" | "children"> {
  /** Becomes the `aria-label`: an icon-only button has no other name. */
  label: string;
  icon: ReactNode;
  variant?: "default" | "ghost";
  size?: "md" | "sm";
  disabled?: boolean;
}

const VARIANT = {
  default: "bg-surface-control shadow-(--gr-highlight) gr-hover:bg-surface-hover gr-pressed:bg-surface-hover gr-pressed:shadow-none",
  ghost: "gr-hover:bg-surface-hover gr-pressed:bg-surface-hover",
} as const;

const SIZE = {
  md: "size-(--gr-size-target-action) rounded-md",
  sm: "size-(--gr-size-target-min) rounded-sm",
} as const;

/** A square button with only an icon. `aria-expanded`, `aria-pressed` and `aria-controls` come from the caller. */
export function IconButton({
  label,
  icon,
  variant = "default",
  size = "md",
  disabled = false,
  className,
  onClick,
  ref,
  type = "button",
  ...rest
}: IconButtonProps) {
  const handleClick = (e: MouseEvent<HTMLButtonElement>) => {
    if (disabled) {
      e.preventDefault();
      return;
    }
    onClick?.(e);
  };
  return (
    <button
      {...rest}
      ref={ref}
      type={type}
      aria-label={label}
      aria-disabled={disabled ? "true" : undefined}
      onClick={handleClick}
      className={cx(
        "inline-flex shrink-0 items-center justify-center text-text-muted cursor-pointer gr-ring gr-motion gr-hover:text-text-main aria-pressed:text-accent-text aria-expanded:text-accent-text aria-disabled:text-text-disabled aria-disabled:cursor-not-allowed",
        SIZE[size],
        VARIANT[variant],
        className,
      )}
    >
      {icon}
    </button>
  );
}
