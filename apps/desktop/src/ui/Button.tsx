import type { ComponentPropsWithRef, MouseEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { cx } from "./cx";

export type ButtonVariant = "primary" | "secondary" | "danger" | "link";
export type ButtonSize = "md" | "sm";

export interface ButtonProps extends Omit<ComponentPropsWithRef<"button">, "disabled"> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  /** Only the "Integrar cambios" CTA. */
  shape?: "pill";
  /** Sets `aria-disabled`, never the attribute: the button stays focusable (it can explain itself). */
  disabled?: boolean;
  /** `aria-busy` + `aria-disabled` + a screen-reader-only "in progress". */
  pending?: boolean;
  icon?: ReactNode;
  /** Renders an `<a>` with the same look and rules. `onClick` and `ref` are not forwarded to it. */
  href?: string;
}

const VARIANT: Record<ButtonVariant, string> = {
  primary:
    "bg-accent text-text-on-accent gr-hover:bg-accent-hover gr-pressed:bg-accent-hover aria-disabled:bg-surface-hover aria-disabled:text-text-disabled",
  secondary:
    "bg-surface-control shadow-(--gr-highlight) border border-border-strong text-text-main gr-hover:bg-surface-hover gr-pressed:bg-surface-hover gr-pressed:shadow-none aria-disabled:border-border-subtle aria-disabled:text-text-disabled",
  danger:
    "bg-surface-control border border-danger text-danger-text gr-hover:bg-linear-to-b gr-hover:from-danger-bg gr-hover:to-danger-bg gr-pressed:bg-linear-to-b gr-pressed:from-danger-bg gr-pressed:to-danger-bg aria-disabled:border-border-subtle aria-disabled:text-text-disabled",
  link: "bg-transparent text-accent-text gr-hover:underline gr-pressed:text-text-main gr-pressed:underline aria-disabled:text-text-disabled aria-disabled:no-underline",
};

const SIZE: Record<ButtonSize, string> = {
  md: "h-(--gr-size-target-action) px-(--gr-space-3) gap-(--gr-space-2) [font:var(--gr-type-body-strong)]",
  sm: "h-(--gr-size-target-min) px-(--gr-space-2) gap-(--gr-space-1) [font:var(--gr-type-secondary)]",
};

/** Four variants, two sizes; state looks come from the shared `gr-*` variants and utilities (D8). */
export function Button({
  variant = "secondary",
  size = "md",
  shape,
  disabled = false,
  pending = false,
  icon,
  href,
  className,
  children,
  onClick,
  ref,
  type = "button",
  ...rest
}: ButtonProps) {
  const { t } = useTranslation();
  const inactive = disabled || pending;
  const radius = size === "sm" ? "rounded-sm" : shape === "pill" ? "rounded-pill" : "rounded-md";
  const classes = cx(
    "inline-flex shrink-0 items-center justify-center whitespace-nowrap select-none no-underline cursor-pointer gr-ring gr-motion aria-disabled:cursor-not-allowed",
    radius,
    SIZE[size],
    VARIANT[variant],
    className,
  );
  const content = (
    <>
      {icon}
      {children}
      {pending ? <span className="sr-only">{t("ui.button.busy")}</span> : null}
    </>
  );

  if (href !== undefined) {
    return (
      <a
        id={rest.id}
        title={rest.title}
        aria-label={rest["aria-label"]}
        aria-describedby={rest["aria-describedby"]}
        href={inactive ? undefined : href}
        aria-disabled={inactive ? "true" : undefined}
        aria-busy={pending ? "true" : undefined}
        onClick={inactive ? (e) => e.preventDefault() : undefined}
        className={classes}
      >
        {content}
      </a>
    );
  }

  const handleClick = (e: MouseEvent<HTMLButtonElement>) => {
    if (inactive) {
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
      aria-disabled={inactive ? "true" : undefined}
      aria-busy={pending ? "true" : undefined}
      onClick={handleClick}
      className={classes}
    >
      {content}
    </button>
  );
}
