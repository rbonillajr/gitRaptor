// TS-DSK-003 T007: StatusPill is a polite live region with a hidden glyph, a visible label and a tone.
import { cleanup, render, screen } from "@testing-library/react";
import axe from "axe-core";
import { afterEach, describe, expect, it } from "vitest";
import { StatusPill, type StatusPillTone } from "./StatusPill";

afterEach(cleanup);
const TONES: StatusPillTone[] = ["success", "warning", "danger", "muted"];

describe("StatusPill", () => {
  it("is a polite status region whose text is visible and whose glyph is hidden from assistive tech", () => {
    render(<StatusPill symbol="●" tone="success" label="Live" />);
    const pill = screen.getByRole("status");
    expect(pill).toHaveAttribute("aria-live", "polite");
    expect(pill).toHaveTextContent("Live");
    expect(screen.getByText("●")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByText("Live", { exact: false })).toBeVisible();
  });

  it.each(TONES)("exposes the %s tone as data-tone", (tone) => {
    render(<StatusPill symbol="◌" tone={tone} label="x" />);
    expect(screen.getByRole("status")).toHaveAttribute("data-tone", tone);
  });

  it("has no axe violations in any tone", async () => {
    const { container } = render(
      <div>
        {TONES.map((tone) => (
          <StatusPill key={tone} symbol="●" tone={tone} label={tone} />
        ))}
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
