/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// TS003-OVERLAYS (TS-DSK-003 T007, D14, ADR-GRP-004 § 3): Dialog confirm opens with the focus on Cancel and Enter
// there never confirms; Tab trap, Escape, focus return, inert siblings, dismissible, info, dev-time throws.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { useRef, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Dialog, type DialogProps } from "./Dialog";

type Extra = Partial<Pick<DialogProps, "dismissible" | "showClose">>;

function ConfirmHarness({ onConfirm, onOpenChange, extra }: { onConfirm: () => void; onOpenChange?: (o: boolean) => void; extra?: Extra }) {
  const [open, setOpen] = useState(false);
  const returnRef = useRef<HTMLButtonElement>(null);
  return (
    <div id="app">
      <button type="button" ref={returnRef} onClick={() => setOpen(true)}>Delete branch</button>
      <Dialog
        open={open}
        onOpenChange={(o: boolean) => { setOpen(o); onOpenChange?.(o); }}
        kind="confirm"
        title="Delete feat-pagos?"
        description="The branch will be removed from this repository."
        confirm={{ label: "Delete", variant: "danger", onConfirm }}
        returnFocusRef={returnRef}
        {...extra}
      />
    </div>
  );
}
const open = () => fireEvent.click(screen.getByRole("button", { name: "Delete branch" }));
const press = (key: string, init: KeyboardEventInit = {}) => fireEvent.keyDown(document.activeElement!, { key, ...init });
const isInert = (el: Element) => el.hasAttribute("inert") || (el as HTMLElement & { inert?: boolean }).inert === true;
const scrim = () => document.querySelector(".bg-surface-scrim");

beforeEach(() => act(async () => void (await i18n.changeLanguage("en"))));
afterEach(() => {
  cleanup();
  document.body.style.overflow = "";
});

describe("TS003-OVERLAYS Dialog confirm", () => {
  it("TS003-OVERLAYS: confirm is an alertdialog named by its title, described by its description, aria-modal", () => {
    render(<ConfirmHarness onConfirm={() => {}} />);
    open();
    const dlg = screen.getByRole("alertdialog", { name: "Delete feat-pagos?" });
    expect(dlg).toHaveAccessibleDescription("The branch will be removed from this repository.");
    expect(dlg).toHaveAttribute("aria-modal", "true");
  });

  it("TS003-OVERLAYS: opens with the focus on Cancel, which comes before the action in the DOM", () => {
    render(<ConfirmHarness onConfirm={() => {}} />);
    open();
    const cancel = screen.getByRole("button", { name: "Cancel" });
    const action = screen.getByRole("button", { name: "Delete" });
    expect(document.activeElement, "initial focus is on Cancel").toBe(cancel);
    expect(cancel.compareDocumentPosition(action) & Node.DOCUMENT_POSITION_FOLLOWING, "Cancel precedes the action").toBeTruthy();
  });

  it("TS003-OVERLAYS: Enter with the focus on Cancel does not confirm and is not intercepted", () => {
    const onConfirm = vi.fn();
    const onOpenChange = vi.fn();
    render(<ConfirmHarness onConfirm={onConfirm} onOpenChange={onOpenChange} />);
    open();
    const notPrevented = fireEvent.keyDown(document.activeElement!, { key: "Enter" });
    expect(notPrevented, "Enter is never intercepted (it activates the focused button)").toBe(true);
    expect(onConfirm, "Enter on Cancel must not call onConfirm").not.toHaveBeenCalled();
    fireEvent.click(document.activeElement!);
    expect(onConfirm, "activating the focused Cancel must not confirm").not.toHaveBeenCalled();
    expect(onOpenChange, "activating Cancel closes the dialog").toHaveBeenLastCalledWith(false);
  });

  it("TS003-OVERLAYS: the action button calls onConfirm", () => {
    const onConfirm = vi.fn();
    render(<ConfirmHarness onConfirm={onConfirm} />);
    open();
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("TS003-OVERLAYS: Tab and Shift+Tab cycle inside the dialog", () => {
    render(<ConfirmHarness onConfirm={() => {}} />);
    open();
    act(() => screen.getByRole("button", { name: "Delete" }).focus());
    press("Tab");
    expect(document.activeElement, "Tab on the last wraps to Cancel").toBe(screen.getByRole("button", { name: "Cancel" }));
    press("Tab", { shiftKey: true });
    expect(document.activeElement, "Shift+Tab on the first wraps to the action").toBe(screen.getByRole("button", { name: "Delete" }));
  });

  it("TS003-OVERLAYS: Escape closes and the focus returns to the trigger", () => {
    render(<ConfirmHarness onConfirm={() => {}} />);
    act(() => screen.getByRole("button", { name: "Delete branch" }).focus());
    open();
    press("Escape");
    expect(screen.queryByRole("alertdialog"), "Escape closes").toBeNull();
    expect(document.activeElement, "focus back on the trigger").toBe(screen.getByRole("button", { name: "Delete branch" }));
  });

  it("TS003-OVERLAYS: a scrim click closes; dismissible={false} ignores Escape and the scrim, Cancel still closes", () => {
    const onOpenChange = vi.fn();
    const { unmount } = render(<ConfirmHarness onConfirm={() => {}} onOpenChange={onOpenChange} />);
    open();
    expect(scrim(), "the scrim exists").not.toBeNull();
    fireEvent.click(scrim()!);
    expect(screen.queryByRole("alertdialog"), "scrim click closes").toBeNull();
    unmount();
    const spy = vi.fn();
    render(<ConfirmHarness onConfirm={() => {}} onOpenChange={spy} extra={{ dismissible: false }} />);
    open();
    press("Escape");
    fireEvent.click(scrim()!);
    expect(screen.getByRole("alertdialog"), "non-dismissible ignores Escape and scrim").toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog"), "Cancel always closes").toBeNull();
  });

  it("TS003-OVERLAYS: siblings of the portal are inert while open and restored on close; the body scroll locks", () => {
    render(<ConfirmHarness onConfirm={() => {}} />);
    const app = document.getElementById("app")!;
    expect(isInert(app)).toBe(false);
    open();
    expect(isInert(app), "the app behind the dialog is inert").toBe(true);
    expect(document.body.style.overflow, "body scroll is locked").toBe("hidden");
    press("Escape");
    expect(isInert(app), "inert is restored after closing").toBe(false);
    expect(document.body.style.overflow, "scroll lock is released").not.toBe("hidden");
  });

  it("TS003-OVERLAYS: confirm without a confirm prop throws in development", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<Dialog open onOpenChange={() => {}} kind="confirm" title="T" description="D" />)).toThrow();
  });
});

describe("TS003-OVERLAYS Dialog info", () => {
  const info = (onAcknowledge = vi.fn(), onOpenChange = vi.fn()) =>
    render(
      <Dialog open onOpenChange={onOpenChange} kind="info" title="Engine stopped" description="Restart it from Settings."
        acknowledge={{ label: "Got it", onAcknowledge }} />,
    );

  it("TS003-OVERLAYS: info is a dialog with the focus on the acknowledge button and a Close button", () => {
    info();
    expect(screen.getByRole("dialog", { name: "Engine stopped" })).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(document.activeElement, "focus on Got it").toBe(screen.getByRole("button", { name: "Got it" }));
    expect(screen.getByRole("button", { name: "Close" }), "info shows the close button by default").toBeInTheDocument();
  });

  it("TS003-OVERLAYS: acknowledging calls onAcknowledge", () => {
    const onAcknowledge = vi.fn();
    info(onAcknowledge);
    fireEvent.click(screen.getByRole("button", { name: "Got it" }));
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
  });

  it("TS003-OVERLAYS: info without acknowledge throws in development", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<Dialog open onOpenChange={() => {}} kind="info" title="T" description="D" />)).toThrow();
  });
});

describe("TS003-OVERLAYS Dialog accessibility", () => {
  it.each(["en", "es"] as const)("TS003-OVERLAYS: no axe violations in %s (confirm and info open)", async (lang) => {
    await act(async () => void (await i18n.changeLanguage(lang)));
    const { unmount } = render(<ConfirmHarness onConfirm={() => {}} />);
    open();
    const confirm = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    expect(confirm.violations.map((v) => `${v.id}: ${v.help}`), "confirm").toEqual([]);
    unmount();
    render(<Dialog open onOpenChange={() => {}} kind="info" title="Engine stopped" description="Restart it." acknowledge={{ label: "Got it" }} />);
    const info = await axe.run(document.body, { rules: { "color-contrast": { enabled: false } } });
    expect(info.violations.map((v) => `${v.id}: ${v.help}`), "info").toEqual([]);
  });
});
