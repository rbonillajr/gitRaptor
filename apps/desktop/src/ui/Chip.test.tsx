// TS003-BASICS (TS-DSK-003 T004): Chip static / toggle / removable, en/es, axe.
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Chip } from "./Chip";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
beforeEach(() => setLang("en"));
afterEach(cleanup);

describe("TS003-BASICS Chip", () => {
  it("TS003-BASICS: a static chip shows its label and exposes no interactive role", () => {
    render(<Chip label="main" />);
    expect(screen.getByText("main")).toBeInTheDocument();
    expect(screen.queryByRole("button"), "a static chip must not be a button").toBeNull();
  });

  it("TS003-BASICS: onToggle renders a button with aria-pressed and calls back with the next value", () => {
    const onToggle = vi.fn();
    const { rerender } = render(<Chip label="Mine" onToggle={onToggle} selected={false} />);
    const b = screen.getByRole("button", { name: "Mine" });
    expect(b.tagName, "native button gives Enter and Space").toBe("BUTTON");
    expect(b).toHaveAttribute("aria-pressed", "false");
    expect(b.querySelector("svg"), "no Check when unselected").toBeNull();
    fireEvent.click(b);
    expect(onToggle).toHaveBeenCalledWith(true);
    rerender(<Chip label="Mine" onToggle={onToggle} selected />);
    const on = screen.getByRole("button", { name: "Mine" });
    expect(on).toHaveAttribute("aria-pressed", "true");
    expect(on.querySelector("svg"), "Check glyph before the label when selected").not.toBeNull();
    fireEvent.click(on);
    expect(onToggle).toHaveBeenLastCalledWith(false);
  });

  it("TS003-BASICS: onRemove adds a 'Remove HEAD' button (es: 'Quitar HEAD') that calls onRemove", async () => {
    const onRemove = vi.fn();
    render(<Chip label="HEAD" onRemove={onRemove} />);
    fireEvent.click(screen.getByRole("button", { name: "Remove HEAD" }));
    expect(onRemove).toHaveBeenCalledTimes(1);
    await setLang("es");
    expect(screen.getByRole("button", { name: "Quitar HEAD" })).toBeInTheDocument();
  });

  it("TS003-BASICS: Delete and Backspace on the chip button remove it; the remove button is a sibling, not nested", () => {
    const onRemove = vi.fn();
    render(<Chip label="HEAD" onToggle={() => {}} onRemove={onRemove} />);
    const chip = screen.getByRole("button", { name: "HEAD" });
    fireEvent.keyDown(chip, { key: "Delete" });
    fireEvent.keyDown(chip, { key: "Backspace" });
    expect(onRemove, "Delete and Backspace must call onRemove").toHaveBeenCalledTimes(2);
    expect(within(chip).queryByRole("button"), "no interactive element inside the chip button").toBeNull();
  });

  it("TS003-BASICS: disabled chip is aria-disabled and does not toggle", () => {
    const onToggle = vi.fn();
    render(<Chip label="Mine" onToggle={onToggle} disabled />);
    const b = screen.getByRole("button", { name: "Mine" });
    expect(b).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(b);
    expect(onToggle).not.toHaveBeenCalled();
  });

  it.each(["en", "es"] as const)("TS003-BASICS: no axe violations in %s", async (lang) => {
    await setLang(lang);
    const { container } = render(
      <div>
        <Chip label="static" />
        <Chip label="mono" mono />
        <Chip label="toggle" onToggle={() => {}} selected />
        <Chip label="removable" onRemove={() => {}} />
        <Chip label="both" onToggle={() => {}} onRemove={() => {}} />
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
