/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-OVERLAYS (TS-DSK-003 T007, D13): Menu opening keys, roving focus, type-ahead, item kinds, Escape and Tab.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { ComponentPropsWithRef } from "react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Menu, type MenuEntry } from "./Menu";

function entries(s: { fetchAll: () => void; pull: () => void; byDate: () => void; tags: (c: boolean) => void; prune: () => void }): MenuEntry[] {
  return [
    { type: "item", id: "fetch", label: "Fetch all", onSelect: s.fetchAll },
    { type: "item", id: "pull", label: "Pull", disabled: true, disabledReason: "Available in UI-2", onSelect: s.pull },
    { type: "radio", id: "date", label: "Sort by date", checked: true, onSelect: s.byDate },
    { type: "radio", id: "name", label: "Sort by name", checked: false, onSelect: () => {} },
    { type: "separator", id: "sep" },
    { type: "checkbox", id: "tags", label: "Show tags", checked: false, onCheckedChange: s.tags },
    { type: "group", id: "more", label: "More", entries: [{ type: "item", id: "prune", label: "Prune", onSelect: s.prune }] },
  ];
}
const spies = () => ({ fetchAll: vi.fn(), pull: vi.fn(), byDate: vi.fn(), tags: vi.fn(), prune: vi.fn() });

function Harness({ s = spies() }: { s?: ReturnType<typeof spies> }) {
  return (
    <Menu label="Fetch options" entries={entries(s)} trigger={(props: ComponentPropsWithRef<"button">) => <button type="button" {...props}>Fetch</button>} />
  );
}
const trigger = () => screen.getByRole("button", { name: "Fetch" });
const menu = () => screen.queryByRole("menu");
const active = () => document.activeElement?.textContent ?? "";
const press = (key: string) => fireEvent.keyDown(document.activeElement!, { key });
const openWith = (key: string) => {
  act(() => trigger().focus());
  fireEvent.keyDown(trigger(), { key });
};

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(cleanup);

describe("TS003-OVERLAYS Menu", () => {
  it("TS003-OVERLAYS: the trigger has aria-haspopup=menu and aria-expanded follows the state", () => {
    render(<Harness />);
    expect(trigger()).toHaveAttribute("aria-haspopup", "menu");
    expect(trigger()).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger());
    expect(trigger()).toHaveAttribute("aria-expanded", "true");
    expect(menu(), "the menu has its label").toHaveAccessibleName("Fetch options");
  });

  it.each(["Enter", " ", "ArrowDown"])("TS003-OVERLAYS: %j on the trigger opens the menu on the first item", (key) => {
    render(<Harness />);
    openWith(key);
    expect(menu(), `${JSON.stringify(key)} must open the menu`).not.toBeNull();
    expect(active(), "the first item takes the focus").toContain("Fetch all");
  });

  it("TS003-OVERLAYS: ArrowUp on the trigger opens on the last item", () => {
    render(<Harness />);
    openWith("ArrowUp");
    expect(active(), "the last item takes the focus").toContain("Prune");
  });

  it("TS003-OVERLAYS: item roles: menuitem, menuitemradio and menuitemcheckbox with their state", () => {
    render(<Harness />);
    fireEvent.click(trigger());
    expect(screen.getByRole("menuitem", { name: "Fetch all" })).toBeInTheDocument();
    expect(screen.getByRole("menuitemradio", { name: "Sort by date" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByRole("menuitemradio", { name: "Sort by name" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("menuitemcheckbox", { name: "Show tags" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("separator")).toBeInTheDocument();
  });

  it("TS003-OVERLAYS: arrows loop through items including disabled ones; Home and End jump", () => {
    render(<Harness />);
    openWith("Enter");
    press("ArrowDown");
    expect(active(), "a disabled item is focusable").toContain("Pull");
    press("End");
    expect(active()).toContain("Prune");
    press("ArrowDown");
    expect(active(), "wrap to the first").toContain("Fetch all");
    press("ArrowUp");
    expect(active(), "wrap to the last").toContain("Prune");
    press("Home");
    expect(active()).toContain("Fetch all");
  });

  it("TS003-OVERLAYS: type-ahead jumps to the next label that starts with the typed letter", () => {
    render(<Harness />);
    openWith("Enter");
    press("s");
    expect(active(), "'s' jumps to Sort by date").toContain("Sort by date");
  });

  it("TS003-OVERLAYS: choosing an item or a radio closes the menu; a checkbox keeps it open", () => {
    const s = spies();
    render(<Harness s={s} />);
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("menuitem", { name: "Fetch all" }));
    expect(s.fetchAll).toHaveBeenCalledTimes(1);
    expect(menu(), "an item closes the menu").toBeNull();
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("menuitemradio", { name: "Sort by date" }));
    expect(s.byDate).toHaveBeenCalledTimes(1);
    expect(menu(), "a radio closes the menu").toBeNull();
    fireEvent.click(trigger());
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "Show tags" }));
    expect(s.tags).toHaveBeenCalledWith(true);
    expect(menu(), "a checkbox keeps the menu open").not.toBeNull();
  });

  it("TS003-OVERLAYS: Escape closes the menu and returns the focus to the trigger", () => {
    render(<Harness />);
    openWith("Enter");
    press("Escape");
    expect(menu()).toBeNull();
    expect(document.activeElement, "the focus returns to the trigger").toBe(trigger());
  });

  it("TS003-OVERLAYS: Tab closes the menu, returns the focus to the trigger and is default-prevented", () => {
    render(<Harness />);
    openWith("Enter");
    const notPrevented = fireEvent.keyDown(document.activeElement!, { key: "Tab" });
    expect(notPrevented, "Tab must preventDefault (D13)").toBe(false);
    expect(menu()).toBeNull();
    expect(document.activeElement).toBe(trigger());
  });

  it("TS003-OVERLAYS: a disabled item is focusable, does not activate and shows its reason", () => {
    const s = spies();
    render(<Harness s={s} />);
    openWith("Enter");
    press("ArrowDown");
    const pull = screen.getByRole("menuitem", { name: "Pull" });
    expect(pull).toHaveAttribute("aria-disabled", "true");
    expect(document.activeElement).toBe(pull);
    fireEvent.click(pull);
    fireEvent.keyDown(pull, { key: "Enter" });
    expect(s.pull, "a disabled item must not run its action").not.toHaveBeenCalled();
    expect(menu(), "a disabled item does not close the menu").not.toBeNull();
    expect(screen.getByRole("tooltip"), "the reason is shown with a tooltip").toHaveTextContent("Available in UI-2");
  });

  it.each(["en", "es"] as const)("TS003-OVERLAYS: no axe violations in %s with the menu open", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    render(<Harness />);
    fireEvent.click(trigger());
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
