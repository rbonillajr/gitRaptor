// TS003-BASICS (TS-DSK-003 T004): Button role, variants, disabled focusable, pending, href, en/es, axe.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { Button, type ButtonVariant } from "./Button";

const VARIANTS: ButtonVariant[] = ["primary", "secondary", "danger", "link"];
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));

beforeEach(() => setLang("en"));
afterEach(cleanup);

describe("TS003-BASICS Button", () => {
  it("TS003-BASICS: renders a button with its name for every variant and size", () => {
    for (const variant of VARIANTS) {
      for (const size of ["md", "sm"] as const) {
        const { unmount } = render(<Button variant={variant} size={size}>Save</Button>);
        expect(screen.getByRole("button", { name: "Save" }), `${variant}/${size}`).toBeInTheDocument();
        unmount();
      }
    }
  });

  it("TS003-BASICS: uses the shared focus ring and motion utilities", () => {
    render(<Button>Save</Button>);
    const cls = screen.getByRole("button").className;
    expect(cls, "Button needs gr-ring").toMatch(/(^|\s)gr-ring(\s|$)/);
    expect(cls, "Button needs gr-motion").toMatch(/(^|\s)gr-motion(\s|$)/);
  });

  it("TS003-BASICS: disabled sets aria-disabled, not the attribute, stays focusable and ignores clicks", () => {
    const onClick = vi.fn();
    render(<Button disabled onClick={onClick}>Save</Button>);
    const b = screen.getByRole("button", { name: "Save" });
    expect(b, "aria-disabled expected").toHaveAttribute("aria-disabled", "true");
    expect(b.hasAttribute("disabled"), "the disabled attribute removes focus").toBe(false);
    act(() => b.focus());
    expect(document.activeElement, "a disabled Button must be focusable").toBe(b);
    fireEvent.click(b);
    expect(onClick, "onClick must not run when disabled").not.toHaveBeenCalled();
  });

  it("TS003-BASICS: pending sets aria-busy, aria-disabled, the sr-only text in en and es, and blocks clicks", async () => {
    const onClick = vi.fn();
    render(<Button pending onClick={onClick}>Save</Button>);
    const b = screen.getByRole("button");
    expect(b).toHaveAttribute("aria-busy", "true");
    expect(b).toHaveAttribute("aria-disabled", "true");
    expect(b, "sr-only busy text in en").toHaveTextContent("In progress");
    fireEvent.click(b);
    expect(onClick, "pending blocks onClick").not.toHaveBeenCalled();
    await setLang("es");
    expect(screen.getByRole("button"), "sr-only busy text in es").toHaveTextContent("En curso");
  });

  it("TS003-BASICS: enabled click runs onClick", () => {
    const onClick = vi.fn();
    render(<Button onClick={onClick}>Save</Button>);
    fireEvent.click(screen.getByRole("button"));
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("TS003-BASICS: href renders a link; disabled drops the href", () => {
    const { rerender } = render(<Button href="https://example.test/x">Docs</Button>);
    expect(screen.getByRole("link", { name: "Docs" }), "href must render <a>").toHaveAttribute("href", "https://example.test/x");
    rerender(<Button href="https://example.test/x" disabled>Docs</Button>);
    expect(screen.getByText("Docs").closest("a")?.hasAttribute("href"), "a disabled link has no href").toBe(false);
  });

  it.each(["en", "es"] as const)("TS003-BASICS: no axe violations in %s (all variants, disabled, pending, link)", async (lang) => {
    await setLang(lang);
    const { container } = render(
      <div>
        {VARIANTS.map((v) => <Button key={v} variant={v}>{v}</Button>)}
        <Button disabled>off</Button>
        <Button pending>busy</Button>
        <Button href="https://example.test">link</Button>
        <Button shape="pill" variant="primary">pill</Button>
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
