/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-OVERLAYS (TS-DSK-003 T007): Popover is a named non-modal dialog; focus goes to the first Toggle, Escape
// restores it, an outside click closes, Tab is not trapped.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { useState, type ComponentPropsWithRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Popover } from "./Popover";
import { Toggle } from "./Toggle";

function Settings({ onOpenChange }: { onOpenChange?: (o: boolean) => void }) {
  const [a, setA] = useState(true);
  const [b, setB] = useState(false);
  const [c, setC] = useState(true);
  return (
    <div>
      <button type="button" id="outside">outside</button>
      <Popover
        label="Column settings"
        {...(onOpenChange ? { onOpenChange } : {})}
        trigger={(props: ComponentPropsWithRef<"button">) => <button type="button" {...props}>Gear</button>}
      >
        <Toggle label="Author" checked={a} onCheckedChange={setA} />
        <Toggle label="Date" checked={b} onCheckedChange={setB} />
        <Toggle label="Sha" checked={c} onCheckedChange={setC} />
      </Popover>
    </div>
  );
}
const trigger = () => screen.getByRole("button", { name: "Gear" });
const dialog = () => screen.queryByRole("dialog");

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-OVERLAYS Popover", () => {
  it("TS003-OVERLAYS: opens as a named dialog with the trigger's aria state; the focus goes to the first Toggle", () => {
    render(<Settings />);
    expect(trigger()).toHaveAttribute("aria-haspopup", "dialog");
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("dialog", { name: "Column settings" })).toBeInTheDocument();
    expect(document.activeElement, "the first focusable of the content takes the focus").toBe(screen.getByRole("switch", { name: "Author" }));
  });

  it("TS003-OVERLAYS: Escape closes and returns the focus to the trigger", () => {
    const onOpenChange = vi.fn();
    render(<Settings onOpenChange={onOpenChange} />);
    fireEvent.click(trigger());
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(dialog(), "Escape closes the popover").toBeNull();
    expect(document.activeElement, "the focus returns to the trigger").toBe(trigger());
    expect(onOpenChange).toHaveBeenLastCalledWith(false);
  });

  it("TS003-OVERLAYS: a click outside closes without returning the focus to the trigger", () => {
    render(<Settings />);
    fireEvent.click(trigger());
    fireEvent.pointerDown(document.getElementById("outside")!);
    expect(dialog(), "an outside pointerdown closes the popover").toBeNull();
    expect(document.activeElement, "the focus is not pulled back").not.toBe(trigger());
  });

  it("TS003-OVERLAYS: a click inside does not close it and Tab is not trapped", () => {
    render(<Settings />);
    fireEvent.click(trigger());
    fireEvent.pointerDown(screen.getByRole("switch", { name: "Date" }));
    expect(dialog(), "inside pointerdown keeps it open").not.toBeNull();
    act(() => screen.getByRole("switch", { name: "Sha" }).focus());
    const notPrevented = fireEvent.keyDown(document.activeElement!, { key: "Tab" });
    expect(notPrevented, "no focus trap: Tab is not default-prevented").toBe(true);
  });

  it.each(["en", "es"] as const)("TS003-OVERLAYS: no axe violations in %s with the popover open", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    render(<Settings />);
    fireEvent.click(trigger());
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});

describe("D18 consumed Esc (DS-US-DSK-008)", () => {
  it("the Esc that closes it is marked as consumed", () => {
    render(<Settings />);
    fireEvent.click(trigger());
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    act(() => void (document.activeElement ?? document.body).dispatchEvent(event));
    expect(dialog(), "Esc closes the popover").toBeNull();
    expect(event.defaultPrevented, "the Esc it consumed is marked with preventDefault").toBe(true);
  });
});
