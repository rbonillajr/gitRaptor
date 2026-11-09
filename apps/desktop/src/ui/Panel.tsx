import type { ComponentPropsWithRef } from "react";

export interface PanelProps extends ComponentPropsWithRef<"section"> {
  /** The landmark the panel is. */
  as?: "nav" | "main" | "aside" | "section";
  /** Visible title of the panel, rendered as its heading. */
  heading?: string;
}

/**
 * A grafito glass zone: `surface.panel` with the top highlight and the panel radius (DSYS-GRP-002 § 2).
 * Pure: no Tauri, no client layer.
 */
export function Panel({ as: Tag = "section", heading, className, children, ...rest }: PanelProps) {
  return (
    <Tag
      {...rest}
      className={`flex flex-col rounded-lg bg-surface-panel p-(--gr-space-4) shadow-(--gr-shadow-panel) ${className ?? ""}`}
    >
      {heading ? <h2 className="m-0 text-text-main [font:var(--gr-type-body-strong)]">{heading}</h2> : null}
      {children}
    </Tag>
  );
}
