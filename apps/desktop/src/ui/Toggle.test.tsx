// TS003-CONTROLS (TS-DSK-003 T005): Toggle switch with label, description and disabled, axe.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Toggle } from "./Toggle";

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-CONTROLS Toggle", () => {
  it("TS003-CONTROLS: a switch named by its label with aria-checked", () => {
    const { rerender } = render(<Toggle label="Show Sha" checked={false} onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Show Sha" })).toHaveAttribute("aria-checked", "false");
    rerender(<Toggle label="Show Sha" checked onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Show Sha" })).toHaveAttribute("aria-checked", "true");
  });

  it("TS003-CONTROLS: clicking the switch or its label calls onCheckedChange with the next value", () => {
    const onCheckedChange = vi.fn();
    render(<Toggle label="Show Sha" checked={false} onCheckedChange={onCheckedChange} />);
    expect(screen.getByRole("switch").tagName, "native button: Enter and Space activate it").toBe("BUTTON");
    fireEvent.click(screen.getByRole("switch"));
    expect(onCheckedChange).toHaveBeenLastCalledWith(true);
    fireEvent.click(screen.getByText("Show Sha"));
    expect(onCheckedChange, "the label is a <label for> of the switch").toHaveBeenCalledTimes(2);
  });

  it("TS003-CONTROLS: disabled does not change and keeps the switch focusable", () => {
    const onCheckedChange = vi.fn();
    render(<Toggle label="Show Sha" checked={false} onCheckedChange={onCheckedChange} disabled />);
    const sw = screen.getByRole("switch");
    expect(sw).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(sw);
    fireEvent.click(screen.getByText("Show Sha"));
    expect(onCheckedChange, "disabled switch must not change").not.toHaveBeenCalled();
  });

  it("TS003-CONTROLS: the description is linked with aria-describedby", () => {
    render(<Toggle label="Show Sha" description="Commit hash column" checked={false} onCheckedChange={() => {}} />);
    expect(screen.getByRole("switch", { name: "Show Sha" })).toHaveAccessibleDescription("Commit hash column");
  });

  it.each(["en", "es"] as const)("TS003-CONTROLS: no axe violations in %s", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    const { container } = render(
      <div>
        <Toggle label="One" checked onCheckedChange={() => {}} />
        <Toggle label="Two" description="More" checked={false} onCheckedChange={() => {}} labelPosition="start" />
        <Toggle label="Three" checked={false} onCheckedChange={() => {}} disabled />
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
