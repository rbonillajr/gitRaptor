/* eslint-disable @typescript-eslint/no-non-null-assertion -- test helpers assert the DOM they just rendered */
// D7-VIEW (DS-US-DSK-007 T005, D12, D16, § 9.4): the announced exception. A countdown with one figure per second in an
// aria-hidden number, a separate role="status" region that speaks at 10 s, at 5 s and at the outcome, and a role="alert"
// notice when the announced exception is not the one the person copied.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import type { OperationDto, PendingExceptionDto } from "@/client/engine-types";
import type { CopiedException } from "./blocks-model";
import { PendingException } from "./PendingException";

const NOW = Date.UTC(2026, 10, 9, 12, 0, 0);
const FORCE: OperationDto = { kind: "push", remote: "origin", refs: [{ name: "main", change: "force" }] };
const CANCEL = "'/Applications/GitRaptor.app/Contents/MacOS/raptor' 'guard' 'cancel' '/w/tienda'";
const pending = (over: Partial<PendingExceptionDto> = {}): PendingExceptionDto => ({
  appliesAtMs: NOW + 10_000,
  windowMs: 10_000,
  operation: FORCE,
  rules: ["minimum.force-push"],
  cancelCommand: CANCEL,
  ...over,
});
const same: CopiedException = { repoId: "tienda", operation: FORCE, rules: ["minimum.force-push"] };
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const tick = (ms: number) => act(() => void vi.advanceTimersByTime(ms));
const show = (p: PendingExceptionDto = pending(), copied: CopiedException = null) =>
  render(
    <QueryClientProvider client={new QueryClient()}>
      <PendingException pending={p} copied={copied} />
    </QueryClientProvider>,
  );
/** The live region of the countdown: the status that is not the copy feedback of CopyCommand. */
const region = () => {
  const found = screen.getAllByRole("status").filter((n) => n.classList.contains("sr-only") && !/Copied|Copiado/.test(n.textContent ?? "") && n.closest("code") === null);
  return found.find((n) => /\d|Running|Ejecut/.test(n.textContent ?? "")) ?? found[0]!;
};

beforeEach(async () => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
  document.documentElement.removeAttribute("data-theme");
  await setLang("en");
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("D7-VIEW PendingException", () => {
  it("D7-VIEW: shows the announced exception with its operation, rules and how to cancel", () => {
    show();
    const text = document.body.textContent ?? "";
    expect(text, "grd.exc.pending with the seconds left").toMatch(/Exception announced in a terminal: runs in 10 s/);
    expect(text, "the operation").toContain("main");
    expect(text, "the rules it skips").toMatch(/no force-push/);
    expect(text, "grd.exc.cancelHow").toContain("press Ctrl-C in that terminal");
    expect(text, "grd.exc.cancelOr").toContain("Or run this in another terminal:");
    expect(screen.getByText(CANCEL, { selector: "code" }), "the cancel command is copyable").toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
  });

  it("D7-VIEW: without a cancel command it does not offer the second terminal", () => {
    show(pending({ cancelCommand: null }));
    expect(document.body.textContent, "no command, no 'or run this'").not.toContain("Or run this in another terminal:");
    expect(screen.queryByRole("button", { name: "Copy" })).toBeNull();
  });

  it("D7-VIEW: the figure counts down every second in an aria-hidden number", () => {
    show();
    const figure = () => [...document.querySelectorAll('[aria-hidden="true"]')].map((n) => n.textContent?.trim()).filter((v) => /^\d+$/.test(v ?? ""));
    expect(figure(), "the countdown figure is aria-hidden").toContain("10");
    tick(1000);
    expect(figure(), "one figure per second").toContain("9");
    tick(3000);
    expect(figure()).toContain("6");
  });

  it("D7-VIEW: the live region announces at 10 s, 5 s and the outcome, never every second", () => {
    show();
    const seen: string[] = [];
    const sample = () => {
      const text = region().textContent?.trim() ?? "";
      if (text !== "" && seen[seen.length - 1] !== text) seen.push(text);
    };
    sample();
    for (let i = 0; i < 12; i += 1) {
      tick(1000);
      sample();
    }
    expect(seen, "three announcements in 12 s: 10 s, 5 s and the outcome").toHaveLength(3);
    expect(seen[0], "first at 10 s").toMatch(/\b10\b/);
    expect(seen[1], "then at 5 s").toMatch(/\b5\b/);
    expect(seen[2], "then the outcome").toMatch(/Running now/);
  });

  it("D7-VIEW: the number is the only countdown with reduced motion", () => {
    const original = window.matchMedia;
    window.matchMedia = ((query: string) => ({ matches: /reduce/.test(query), media: query, addEventListener: () => {}, removeEventListener: () => {}, addListener: () => {}, removeListener: () => {}, onchange: null, dispatchEvent: () => false })) as typeof window.matchMedia;
    try {
      const { container } = show();
      tick(2000);
      expect(container.querySelector('[class*="animate"], [class*="transition"], [style*="transition"], [style*="animation"], progress, [role="progressbar"]'), "no bar, no animation: only the figure").toBeNull();
      const figure = container.querySelector('[aria-hidden="true"]');
      expect(figure, "the figure is painted").not.toBeNull();
      expect(figure?.textContent, "the figure still counts").toMatch(/8/);
    } finally {
      window.matchMedia = original;
    }
  });

  it("D7-VIEW: a mismatch shows an alert with the cancel command", () => {
    show(pending(), { ...same, rules: ["policy.protected-branch"] });
    const alert = screen.getByRole("alert");
    expect(alert, "grd.exc.mismatch").toHaveTextContent("The announced exception isn't the one you copied: cancel it with Ctrl-C in that terminal.");
    expect(screen.getByText(CANCEL, { selector: "code" }), "the cancel command is there").toBeInTheDocument();
    expect(alert.compareDocumentPosition(screen.getByText(/Exception announced in a terminal/)) & Node.DOCUMENT_POSITION_FOLLOWING, "the alert comes before everything").toBeTruthy();
  });

  it("D7-VIEW: the same exception as the copied one shows no alert", () => {
    show(pending(), same);
    expect(screen.getByText(/Exception announced in a terminal/), "the panel exists").toBeInTheDocument();
    expect(screen.queryByRole("alert"), "no mismatch, no alert").toBeNull();
  });

  it("D7-VIEW: without a copy there is no alert", () => {
    show(pending(), null);
    expect(screen.getByText(/Exception announced in a terminal/), "the panel exists").toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  for (const lang of ["en", "es"] as const) {
    for (const theme of ["light", "dark"] as const) {
      it(`D7-VIEW: axe finds no violations (${lang}, ${theme})`, async () => {
        vi.useRealTimers();
        await setLang(lang);
        document.documentElement.setAttribute("data-theme", theme);
        const { container } = show(pending({ appliesAtMs: Date.now() + 10_000 }), { ...same, rules: ["x"] });
        expect(within(container).getByRole("alert"), "painted before axe").toBeInTheDocument();
        const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
        expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
      });
    }
  }
});
