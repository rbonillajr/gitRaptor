import type { ReactNode } from "react";

/** Glyph size: `md` = `--gr-space-4`, `sm` = `--gr-space-3`. */
export interface IconProps {
  size?: "sm" | "md";
}

function Glyph({ size = "md", children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.5}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      className={size === "md" ? "size-(--gr-space-4)" : "size-(--gr-space-3)"}
    >
      {children}
    </svg>
  );
}

export const ChevronRight = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <path d="M6 4l4 4-4 4" />
  </Glyph>
);

export const Close = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <path d="M4 4l8 8M12 4l-8 8" />
  </Glyph>
);

export const Check = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <path d="M3.5 8.5l3 3 6-7" />
  </Glyph>
);

export const Undo = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <path d="M6 3.5L3 6.5l3 3" />
    <path d="M3 6.5h6a3.5 3.5 0 010 7H6" />
  </Glyph>
);

export const CircleAlert = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <circle cx="8" cy="8" r="6" />
    <path d="M8 5v3.5M8 11h.01" />
  </Glyph>
);

export const OctagonX = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <path d="M5.2 2h5.6L14 5.2v5.6L10.8 14H5.2L2 10.8V5.2z" />
    <path d="M6 6l4 4M10 6l-4 4" />
  </Glyph>
);

export const Gear = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <circle cx="8" cy="8" r="2" />
    <path d="M8 1.5v2M8 12.5v2M2.4 4.75l1.7 1M11.9 10.25l1.7 1M2.4 11.25l1.7-1M11.9 5.75l1.7-1" />
  </Glyph>
);

export const Dot = ({ size }: IconProps) => (
  <Glyph {...(size ? { size } : {})}>
    <circle cx="8" cy="8" r="4" fill="currentColor" stroke="none" />
  </Glyph>
);
