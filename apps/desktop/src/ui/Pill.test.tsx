// TS003-BASICS (TS-DSK-003 T004): Pill labels per kind in en/es, announce -> role=status, axe.
import { act, cleanup, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, describe, expect, it } from "vitest";
import "@/i18n";
import { Pill, type PillKind } from "./Pill";

const LABELS: Record<"en" | "es", Record<PillKind, string>> = {
  en: { reversible: "Recoverable", confirm: "Needs confirmation", irreversible: "Can't be undone", live: "Live" },
  es: { reversible: "Recuperable", confirm: "Pide confirmación", irreversible: "No se puede deshacer", live: "En vivo" },
};
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
afterEach(cleanup);

describe("TS003-BASICS Pill", () => {
  it.each(["en", "es"] as const)("TS003-BASICS: shows the label of every kind in %s", async (lang) => {
    await setLang(lang);
    for (const kind of Object.keys(LABELS[lang]) as PillKind[]) {
      const { unmount } = render(<Pill kind={kind} />);
      expect(screen.getByText(LABELS[lang][kind]), `${lang}/${kind}`).toBeInTheDocument();
      unmount();
    }
  });

  it("TS003-BASICS: announce sets role=status; without it there is no role", async () => {
    await setLang("en");
    const { rerender } = render(<Pill kind="reversible" announce />);
    expect(screen.getByRole("status")).toHaveTextContent("Recoverable");
    rerender(<Pill kind="reversible" />);
    expect(screen.queryByRole("status"), "no live region without announce").toBeNull();
  });

  it.each(["en", "es"] as const)("TS003-BASICS: no axe violations in %s", async (lang) => {
    await setLang(lang);
    const { container } = render(
      <div>
        {(Object.keys(LABELS.en) as PillKind[]).map((k) => <Pill key={k} kind={k} />)}
        <Pill kind="live" announce />
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
