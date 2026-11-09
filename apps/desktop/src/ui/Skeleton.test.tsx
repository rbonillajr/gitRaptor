// TS-DSK-003 T007: Skeleton announces "Loading…" and draws decorative bars.
import { act, cleanup, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, describe, expect, it } from "vitest";
import "@/i18n";
import { Skeleton } from "./Skeleton";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
afterEach(cleanup);

describe("Skeleton", () => {
  it.each([
    ["en", "Loading…"],
    ["es", "Cargando…"],
  ] as const)("is a busy status that says loading in %s", async (lang, text) => {
    await setLang(lang);
    render(<Skeleton />);
    const region = screen.getByRole("status");
    expect(region).toHaveAttribute("aria-busy", "true");
    expect(region).toHaveTextContent(text);
  });

  it("draws the requested number of hidden bars", () => {
    const { container } = render(<Skeleton lines={5} />);
    expect(container.querySelectorAll('[aria-hidden="true"]')).toHaveLength(5);
  });

  it("has no axe violations", async () => {
    await setLang("en");
    const { container } = render(<Skeleton />);
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
  });
});
