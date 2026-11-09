// TS003-BASICS (TS-DSK-003 T004): IconButton name, state props, ghost, disabled, en/es axe.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { IconButton } from "./IconButton";

const icon = <svg aria-hidden="true" />;
beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-BASICS IconButton", () => {
  it("TS003-BASICS: aria-label is the label", () => {
    render(<IconButton label="Left panel" icon={icon} />);
    expect(screen.getByRole("button", { name: "Left panel" })).toBeInTheDocument();
  });

  it("TS003-BASICS: aria-expanded and aria-pressed from the caller reach the button", () => {
    render(<IconButton label="Panel" icon={icon} aria-expanded aria-pressed="true" aria-controls="x" />);
    const b = screen.getByRole("button", { name: "Panel" });
    expect(b).toHaveAttribute("aria-expanded", "true");
    expect(b).toHaveAttribute("aria-pressed", "true");
    expect(b).toHaveAttribute("aria-controls", "x");
  });

  it("TS003-BASICS: ghost has no background at rest; default does; both use gr-ring", () => {
    const { rerender } = render(<IconButton label="A" icon={icon} variant="ghost" />);
    const bg = /(^|\s)bg-/;
    expect(screen.getByRole("button").className, "ghost must have no resting bg-*").not.toMatch(bg);
    rerender(<IconButton label="A" icon={icon} />);
    expect(screen.getByRole("button").className, "default has a resting background").toMatch(bg);
    expect(screen.getByRole("button").className).toMatch(/(^|\s)gr-ring(\s|$)/);
  });

  it("TS003-BASICS: disabled is aria-disabled, focusable and ignores clicks", () => {
    const onClick = vi.fn();
    render(<IconButton label="A" icon={icon} disabled onClick={onClick} />);
    const b = screen.getByRole("button");
    expect(b).toHaveAttribute("aria-disabled", "true");
    expect(b.hasAttribute("disabled")).toBe(false);
    fireEvent.click(b);
    expect(onClick).not.toHaveBeenCalled();
  });

  it.each(["en", "es"] as const)("TS003-BASICS: no axe violations in %s", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    const { container } = render(
      <div>
        <IconButton label="One" icon={icon} />
        <IconButton label="Two" icon={icon} variant="ghost" size="sm" />
        <IconButton label="Three" icon={icon} disabled />
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
