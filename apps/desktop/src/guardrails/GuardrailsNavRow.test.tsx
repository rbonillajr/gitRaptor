// D7-VIEW (DS-US-DSK-007 T005, D18, § 9.4): the sidebar footer row. A button "Guardrails" with the block symbol and the
// unseen count in a danger badge; no badge without unseen blocks; aria-current when its view is open.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { GuardrailsNavRow } from "./GuardrailsNavRow";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const show = (unseen: number, active = false, onOpen: () => void = () => {}) => render(<GuardrailsNavRow unseen={unseen} active={active} onOpen={onOpen} />);

beforeEach(async () => {
  document.documentElement.removeAttribute("data-theme");
  await setLang("en");
});
afterEach(cleanup);

describe("D7-VIEW GuardrailsNavRow", () => {
  it("D7-VIEW: its name includes Guardrails and the count", async () => {
    show(2);
    expect(screen.getByRole("button", { name: "Guardrails, 2" }), "grd.nav.labelUnseen in en").toBeInTheDocument();
    cleanup();
    await setLang("es");
    show(2);
    expect(screen.getByRole("button", { name: "Guardrails, 2" }), "grd.nav.labelUnseen in es").toBeInTheDocument();
  });

  it("D7-VIEW: the badge says how many new blocks, with the block symbol and text", () => {
    show(2);
    const badge = screen.getByLabelText("2 new blocks");
    expect(badge, "the count in the badge").toHaveTextContent("2");
    expect(screen.getByRole("button").textContent, "the block symbol never goes without text").toMatch(/⛔.*Guardrails|Guardrails.*⛔/);
    cleanup();
    show(1);
    expect(screen.getByLabelText("1 new block"), "singular").toBeInTheDocument();
  });

  it("D7-VIEW: without unseen blocks there is no badge and the name is just Guardrails", () => {
    show(0);
    expect(screen.getByRole("button", { name: "Guardrails" }), "plain name").toBeInTheDocument();
    expect(screen.queryByLabelText(/new block/), "no badge").toBeNull();
  });

  it("D7-VIEW: aria-current only when its view is open", () => {
    show(0, true);
    expect(screen.getByRole("button", { name: /Guardrails/ }), "open view is current").toHaveAttribute("aria-current", expect.not.stringMatching(/^false$/));
    cleanup();
    show(0, false);
    expect(screen.getByRole("button", { name: /Guardrails/ }), "closed view is not current").not.toHaveAttribute("aria-current");
  });

  it("D7-VIEW: a click opens the view", () => {
    const onOpen = vi.fn();
    show(1, false, onOpen);
    fireEvent.click(screen.getByRole("button", { name: /Guardrails/ }));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  for (const lang of ["en", "es"] as const) {
    for (const theme of ["light", "dark"] as const) {
      it(`D7-VIEW: axe finds no violations (${lang}, ${theme})`, async () => {
        await setLang(lang);
        document.documentElement.setAttribute("data-theme", theme);
        const { container } = show(3, true);
        expect(screen.getByRole("button", { name: /Guardrails/ }), "painted before axe").toBeInTheDocument();
        const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
        expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
      });
    }
  }
});
