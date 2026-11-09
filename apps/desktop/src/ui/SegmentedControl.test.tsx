/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-CONTROLS (TS-DSK-003 T005): SegmentedControl radiogroup, one Tab stop, arrows select with wrap, axe.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { SegmentedControl, type SegmentOption } from "./SegmentedControl";

const OPTIONS: SegmentOption<string>[] = [
  { value: "a", label: "Auto" },
  { value: "b", label: "Light" },
  { value: "c", label: "Dark", disabled: true },
  { value: "d", label: "Contrast" },
];

function Harness({ initial = "a", onChange }: { initial?: string; onChange?: (v: string) => void }) {
  const [value, setValue] = useState(initial);
  return (
    <div>
      <button type="button">before</button>
      <SegmentedControl
        label="Theme"
        options={OPTIONS}
        value={value}
        onChange={(v) => {
          setValue(v);
          onChange?.(v);
        }}
      />
    </div>
  );
}
const radio = (name: string) => screen.getByRole("radio", { name });
const press = (key: string) => fireEvent.keyDown(document.activeElement!, { key });
beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-CONTROLS SegmentedControl", () => {
  it("TS003-CONTROLS: radiogroup with its name; the selected option is aria-checked", () => {
    render(<Harness initial="b" />);
    expect(screen.getByRole("radiogroup", { name: "Theme" })).toBeInTheDocument();
    expect(radio("Light")).toHaveAttribute("aria-checked", "true");
    expect(radio("Auto")).toHaveAttribute("aria-checked", "false");
  });

  it("TS003-CONTROLS: only the selected radio is a Tab stop", () => {
    render(<Harness initial="b" />);
    const stops = screen.getAllByRole("radio").filter((r) => r.tabIndex === 0).map((r) => r.textContent);
    expect(stops, "one Tab stop, on the selected segment").toEqual(["Light"]);
  });

  it("TS003-CONTROLS: arrows move the focus and select, wrapping at the ends and skipping disabled", () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    act(() => radio("Auto").focus());
    press("ArrowRight");
    expect(document.activeElement, "ArrowRight focuses the next segment").toBe(radio("Light"));
    expect(onChange, "moving the focus selects").toHaveBeenLastCalledWith("b");
    press("ArrowRight");
    expect(document.activeElement, "the disabled segment is skipped").toBe(radio("Contrast"));
    expect(onChange).toHaveBeenLastCalledWith("d");
    press("ArrowRight");
    expect(document.activeElement, "wrap to the first").toBe(radio("Auto"));
    press("ArrowLeft");
    expect(document.activeElement, "wrap back to the last").toBe(radio("Contrast"));
    press("ArrowDown");
    expect(document.activeElement, "ArrowDown also moves forward (orientation both)").toBe(radio("Auto"));
  });

  it("TS003-CONTROLS: Home and End jump to the first and last enabled segment", () => {
    render(<Harness initial="b" />);
    act(() => radio("Light").focus());
    press("End");
    expect(document.activeElement).toBe(radio("Contrast"));
    press("Home");
    expect(document.activeElement).toBe(radio("Auto"));
  });

  it("TS003-CONTROLS: click selects; a disabled segment does not", () => {
    const onChange = vi.fn();
    render(<Harness onChange={onChange} />);
    fireEvent.click(radio("Dark"));
    expect(onChange, "a disabled segment must not select").not.toHaveBeenCalled();
    fireEvent.click(radio("Light"));
    expect(onChange).toHaveBeenCalledWith("b");
  });

  it.each(["en", "es"] as const)("TS003-CONTROLS: no axe violations in %s", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    const { container } = render(<Harness />);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
