// TS003-OVERLAYS (TS-DSK-003 T007, D12): Tooltip opens on keyboard focus, after 500 ms of hover, instantly inside
// the 300 ms skip window; Escape closes; aria-describedby only for disabled triggers or `describes`.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Tooltip } from "./Tooltip";

const tip = () => screen.queryByRole("tooltip");
const btn = (name: string) => screen.getByRole("button", { name });

beforeEach(async () => {
  await act(async () => void (await i18n.changeLanguage("en")));
  vi.useFakeTimers();
  // Past any skip window left by a previous test (the window is module state).
  act(() => void vi.advanceTimersByTime(10_000));
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("TS003-OVERLAYS Tooltip", () => {
  it("TS003-OVERLAYS: opens at once on keyboard focus and closes on blur", () => {
    render(<Tooltip content="Left panel"><button type="button">Panel</button></Tooltip>);
    expect(tip(), "closed at rest").toBeNull();
    act(() => btn("Panel").focus());
    expect(tip(), "focus opens the tooltip with no delay").toHaveTextContent("Left panel");
    act(() => btn("Panel").blur());
    expect(tip(), "blur closes it").toBeNull();
  });

  it("TS003-OVERLAYS: hover opens after 500 ms, not before", () => {
    render(<Tooltip content="Left panel"><button type="button">Panel</button></Tooltip>);
    fireEvent.pointerEnter(btn("Panel"));
    act(() => void vi.advanceTimersByTime(499));
    expect(tip(), "still closed at 499 ms").toBeNull();
    act(() => void vi.advanceTimersByTime(1));
    expect(tip(), "open at 500 ms").toHaveTextContent("Left panel");
  });

  it("TS003-OVERLAYS: a tooltip opens instantly when another closed less than 300 ms ago", () => {
    render(
      <div>
        <Tooltip content="First"><button type="button">A</button></Tooltip>
        <Tooltip content="Second"><button type="button">B</button></Tooltip>
      </div>,
    );
    fireEvent.pointerEnter(btn("A"));
    act(() => void vi.advanceTimersByTime(500));
    expect(tip()).toHaveTextContent("First");
    fireEvent.pointerLeave(btn("A"));
    act(() => void vi.advanceTimersByTime(150));
    expect(tip(), "closed after the grace period").toBeNull();
    fireEvent.pointerEnter(btn("B"));
    expect(tip(), "inside the skip window the next tooltip opens with no delay").toHaveTextContent("Second");
  });

  it("TS003-OVERLAYS: after the skip window the 500 ms delay applies again", () => {
    render(
      <div>
        <Tooltip content="First"><button type="button">A</button></Tooltip>
        <Tooltip content="Second"><button type="button">B</button></Tooltip>
      </div>,
    );
    fireEvent.pointerEnter(btn("A"));
    act(() => void vi.advanceTimersByTime(500));
    fireEvent.pointerLeave(btn("A"));
    act(() => void vi.advanceTimersByTime(600));
    fireEvent.pointerEnter(btn("B"));
    expect(tip(), "outside the 300 ms window the delay is back").toBeNull();
  });

  it("TS003-OVERLAYS: Escape closes an open tooltip", () => {
    render(<Tooltip content="Left panel"><button type="button">Panel</button></Tooltip>);
    act(() => btn("Panel").focus());
    expect(tip()).not.toBeNull();
    fireEvent.keyDown(btn("Panel"), { key: "Escape" });
    expect(tip(), "Escape dismisses the tooltip").toBeNull();
  });

  it("TS003-OVERLAYS: aria-describedby only for aria-disabled triggers or describes; aria-keyshortcuts from shortcut", () => {
    const { unmount } = render(<Tooltip content="Plain"><button type="button">Plain</button></Tooltip>);
    act(() => btn("Plain").focus());
    expect(btn("Plain").hasAttribute("aria-describedby"), "a labelled trigger needs no description").toBe(false);
    unmount();
    const disabled = render(<Tooltip content="Not now"><button type="button" aria-disabled="true">Off</button></Tooltip>);
    act(() => btn("Off").focus());
    expect(btn("Off"), "an aria-disabled trigger gets the tooltip as its description").toHaveAccessibleDescription(/Not now/);
    disabled.unmount();
    render(<Tooltip content="Why" describes shortcut={{ keys: "⌘B", aria: "Meta+B" }}><button type="button">Info</button></Tooltip>);
    act(() => btn("Info").focus());
    expect(btn("Info")).toHaveAccessibleDescription(/Why/);
    expect(btn("Info"), "aria-keyshortcuts uses shortcut.aria").toHaveAttribute("aria-keyshortcuts", "Meta+B");
  });

  it("TS003-OVERLAYS: the shortcut shows in the tooltip with an sr-only phrase in en and es", async () => {
    render(<Tooltip content="Left panel" shortcut={{ keys: "⌘B", aria: "Meta+B" }}><button type="button">Panel</button></Tooltip>);
    act(() => btn("Panel").focus());
    expect(tip()).toHaveTextContent("⌘B");
    expect(tip(), "sr-only shortcut phrase in en").toHaveTextContent("Shortcut: ⌘B");
    act(() => btn("Panel").blur());
    await act(async () => void (await i18n.changeLanguage("es")));
    act(() => btn("Panel").focus());
    expect(tip(), "sr-only shortcut phrase in es").toHaveTextContent("Atajo: ⌘B");
  });

  it.each(["en", "es"] as const)("TS003-OVERLAYS: no axe violations in %s with the tooltip open", async (lang) => {
    vi.useRealTimers();
    await act(async () => void (await i18n.changeLanguage(lang)));
    render(<Tooltip content="Left panel" shortcut={{ keys: "⌘B", aria: "Meta+B" }}><button type="button">Panel</button></Tooltip>);
    act(() => btn("Panel").focus());
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
