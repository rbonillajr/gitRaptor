// TS003-BASICS (TS-DSK-003 T004): StatusDot role=img named by tone in en/es, decorative, axe.
import { act, cleanup, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, describe, expect, it } from "vitest";
import "@/i18n";
import { StatusDot, type StatusTone } from "./StatusDot";

const NAMES: Record<"en" | "es", Record<StatusTone, string>> = {
  en: { active: "Active", inactive: "Inactive", conflict: "Predicted conflict", live: "Live" },
  es: { active: "Activo", inactive: "Inactivo", conflict: "Conflicto previsto", live: "En vivo" },
};
const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
afterEach(cleanup);

describe("TS003-BASICS StatusDot", () => {
  it.each(["en", "es"] as const)("TS003-BASICS: role=img named by tone in %s", async (lang) => {
    await setLang(lang);
    for (const tone of Object.keys(NAMES[lang]) as StatusTone[]) {
      const { unmount } = render(<StatusDot tone={tone} />);
      expect(screen.getByRole("img", { name: NAMES[lang][tone] }), `${lang}/${tone}`).toBeInTheDocument();
      unmount();
    }
  });

  it("TS003-BASICS: decorative hides the dot from assistive tech", async () => {
    await setLang("en");
    const { container } = render(<StatusDot tone="active" decorative />);
    expect(screen.queryByRole("img"), "a decorative dot has no img role").toBeNull();
    expect(container.firstElementChild, "aria-hidden expected").toHaveAttribute("aria-hidden", "true");
  });

  it.each(["en", "es"] as const)("TS003-BASICS: no axe violations in %s", async (lang) => {
    await setLang(lang);
    const { container } = render(
      <div>
        {(Object.keys(NAMES.en) as StatusTone[]).map((t) => <StatusDot key={t} tone={t} />)}
        <StatusDot tone="live" decorative />
      </div>,
    );
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
