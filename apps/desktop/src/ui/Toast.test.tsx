/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-OVERLAYS (TS-DSK-003 T007): Toast roles, 5 s auto close (not for errors), pause on hover / focus / window
// blur, max 3 visible, F8, no focus theft, action and close.
import { act, cleanup, fireEvent, render, renderHook, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { ToastProvider, useToast, type ToastInput } from "./Toast";

let api!: ReturnType<typeof useToast>;
function Probe() {
  api = useToast();
  return <button type="button">Trigger</button>;
}
const mount = () => render(<ToastProvider><Probe /></ToastProvider>);
const show = (t: ToastInput) => act(() => void api.show(t));
const ms = (n: number) => act(() => void vi.advanceTimersByTime(n));
const region = () => screen.getByRole("region", { name: "Notifications" });

beforeEach(async () => {
  await act(async () => void (await i18n.changeLanguage("en")));
  vi.useFakeTimers();
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("TS003-OVERLAYS Toast", () => {
  it("TS003-OVERLAYS: success, info and warning are role=status; error is role=alert; all inside the labelled region", () => {
    mount();
    show({ kind: "error", message: "Boom" });
    show({ kind: "success", message: "Saved" });
    show({ kind: "warning", message: "Careful" });
    expect(within(region()).getAllByRole("status"), "success and warning are statuses").toHaveLength(2);
    expect(within(region()).getByRole("alert"), "error is an alert").toHaveTextContent("Boom");
  });

  it("TS003-OVERLAYS: the kind prefix is sr-only for success, warning and error, and absent for info; es region name", async () => {
    mount();
    show({ kind: "success", message: "Saved" });
    expect(screen.getByText("Saved").closest('[role="status"]'), "success prefix").toHaveTextContent("Done");
    show({ kind: "warning", message: "Careful" });
    expect(screen.getByText("Careful").closest('[role="status"]'), "warning prefix").toHaveTextContent("Warning");
    show({ kind: "error", message: "Boom" });
    ms(5000);
    expect(screen.getByText("Boom").closest('[role="alert"]'), "error prefix").toHaveTextContent("Error");
    cleanup();
    mount();
    show({ kind: "info", message: "FYI" });
    expect(screen.getByText("FYI").closest('[role="status"]')?.textContent, "info has no kind prefix").not.toMatch(/Done|Warning|Error/);
    await act(async () => void (await i18n.changeLanguage("es")));
    expect(screen.getByRole("region", { name: "Avisos" })).toBeInTheDocument();
  });

  it("TS003-OVERLAYS: success closes at 5000 ms, not before", () => {
    mount();
    show({ kind: "success", message: "Saved" });
    ms(4999);
    expect(screen.queryByText("Saved"), "still visible at 4999 ms").not.toBeNull();
    ms(1);
    expect(screen.queryByText("Saved"), "gone at 5000 ms").toBeNull();
  });

  it("TS003-OVERLAYS: an error toast never closes by itself", () => {
    mount();
    show({ kind: "error", message: "Boom" });
    ms(60_000);
    expect(screen.queryByText("Boom"), "error persists").not.toBeNull();
  });

  it("TS003-OVERLAYS: hovering the viewport pauses the timer and leaving resumes with the time left", () => {
    mount();
    show({ kind: "info", message: "FYI" });
    ms(3000);
    fireEvent.pointerEnter(region());
    ms(20_000);
    expect(screen.queryByText("FYI"), "paused while hovered").not.toBeNull();
    fireEvent.pointerLeave(region());
    ms(1999);
    expect(screen.queryByText("FYI"), "2000 ms were left: still visible at 1999").not.toBeNull();
    ms(1);
    expect(screen.queryByText("FYI"), "closes when the remaining time ends").toBeNull();
  });

  it("TS003-OVERLAYS: focus inside the toast pauses the timer", () => {
    mount();
    show({ kind: "info", message: "FYI", action: { label: "Undo", onAction: () => {} } });
    act(() => screen.getByRole("button", { name: "Undo" }).focus());
    ms(20_000);
    expect(screen.queryByText("FYI"), "paused while the focus is inside").not.toBeNull();
    act(() => screen.getByRole("button", { name: "Undo" }).blur());
    ms(5000);
    expect(screen.queryByText("FYI"), "closes after the focus leaves").toBeNull();
  });

  it("TS003-OVERLAYS: a window without focus pauses the timer; regaining it resumes", () => {
    mount();
    show({ kind: "info", message: "FYI" });
    ms(1000);
    fireEvent.blur(window);
    ms(20_000);
    expect(screen.queryByText("FYI"), "paused while the window is blurred").not.toBeNull();
    fireEvent.focus(window);
    ms(4000);
    expect(screen.queryByText("FYI"), "closes once the 4000 ms left have elapsed").toBeNull();
  });

  it("TS003-OVERLAYS: at most 3 toasts are visible; the rest wait in the queue", () => {
    mount();
    for (const m of ["one", "two", "three", "four", "five"]) show({ kind: "info", message: m });
    expect(screen.queryByText("four"), "the 4th waits").toBeNull();
    expect(screen.queryByText("one")).not.toBeNull();
    expect(screen.queryByText("three")).not.toBeNull();
    ms(5000);
    expect(screen.queryByText("four"), "the 4th appears when a slot frees").not.toBeNull();
  });

  it("TS003-OVERLAYS: a toast does not take the focus; F8 focuses the first toast", () => {
    mount();
    act(() => screen.getByRole("button", { name: "Trigger" }).focus());
    show({ kind: "success", message: "Saved" });
    expect(document.activeElement, "appearing must not steal the focus").toBe(screen.getByRole("button", { name: "Trigger" }));
    fireEvent.keyDown(window, { key: "F8" });
    expect(region().contains(document.activeElement), "F8 moves the focus into the toast region").toBe(true);
  });

  it("TS003-OVERLAYS: the action runs and closes the toast; the close button and Escape close it", () => {
    const onAction = vi.fn();
    mount();
    show({ kind: "success", message: "Merged", action: { label: "Undo", onAction } });
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(onAction).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Merged"), "the action closes the toast").toBeNull();
    show({ kind: "info", message: "Bye" });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByText("Bye"), "Close removes the toast").toBeNull();
    show({ kind: "info", message: "Esc" });
    act(() => screen.getByRole("button", { name: "Close" }).focus());
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByText("Esc"), "Escape inside closes that toast").toBeNull();
  });

  it("TS003-OVERLAYS: dismiss(id) removes a toast", () => {
    mount();
    let id = "";
    act(() => void (id = api.show({ kind: "error", message: "Boom" })));
    act(() => api.dismiss(id));
    expect(screen.queryByText("Boom")).toBeNull();
  });

  it("TS003-OVERLAYS: useToast outside a ToastProvider throws", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => renderHook(() => useToast())).toThrow("useToast needs a ToastProvider");
  });

  it.each(["en", "es"] as const)("TS003-OVERLAYS: no axe violations in %s with one toast of each kind", async (lang) => {
    vi.useRealTimers();
    await act(async () => void (await i18n.changeLanguage(lang)));
    mount();
    for (const kind of ["success", "info", "warning"] as const) show({ kind, message: kind, action: { label: "Act", onAction: () => {} } });
    const results = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});

describe("D18 consumed Esc (DS-US-DSK-008)", () => {
  it("the Esc that closes it is marked as consumed", () => {
    mount();
    show({ kind: "info", message: "FYI", action: { label: "View", onAction: () => {} } });
    const button = screen.getByRole("button", { name: "View" });
    act(() => button.focus());
    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    act(() => void button.dispatchEvent(event));
    expect(screen.queryByText("FYI"), "Esc closes the toast").toBeNull();
    expect(event.defaultPrevented, "the Esc it consumed is marked with preventDefault").toBe(true);
  });
});

describe("D7-DIALOG Toast srLabel (DS-US-DSK-007 D15)", () => {
  it("srLabel replaces the hidden kind prefix", () => {
    mount();
    show({ kind: "error", message: "Boom", srLabel: "Blocked" } as ToastInput);
    const toast = screen.getByText("Boom").closest('[role="alert"]')!;
    expect(toast.querySelector(".sr-only")?.textContent, "the sr-only prefix is the srLabel").toBe("Blocked");
    expect(toast.textContent, "and the generic kind prefix is gone").not.toContain("Error");
  });

  it("without srLabel the prefix is the kind", () => {
    mount();
    show({ kind: "error", message: "Boom" });
    expect(screen.getByText("Boom").closest('[role="alert"]')!.querySelector(".sr-only")?.textContent, "the kind prefix").toBe("Error");
    show({ kind: "warning", message: "Careful" });
    expect(screen.getByText("Careful").closest('[role="status"]')!.querySelector(".sr-only")?.textContent).toBe("Warning");
  });
});
