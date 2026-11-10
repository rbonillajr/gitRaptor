// DSK002-UI (TS-DSK-002 T007, D-A): AutostartOffer is a pure component (props in, callbacks out). With
// `offer: "offered"` it shows the action to enable the autostart, with `"registered"` the one to stop it;
// pressing it calls onChange(true) / onChange(false); `pending` makes it aria-disabled; no axe violations
// in light and dark, in en and es. The confirmation dialog belongs to the backend, never to this component.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import en from "@/i18n/locales/en.json";
import es from "@/i18n/locales/es.json";
import { AutostartOffer } from "./AutostartOffer";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));

beforeEach(() => setLang("en"));
afterEach(cleanup);

/** The one control of the component: a button (or a switch, if the component uses a Toggle). */
function control(): HTMLElement {
  const all = [...screen.queryAllByRole("button"), ...screen.queryAllByRole("switch")];
  expect(all, "DSK002-UI: AutostartOffer must show exactly one control to ask for the change").toHaveLength(1);
  return all[0] as HTMLElement;
}

async function nameOf(offer: "offered" | "registered", lang: "en" | "es"): Promise<string> {
  await setLang(lang);
  const { unmount } = render(<AutostartOffer offer={offer} onChange={() => {}} />);
  const name = control().textContent?.trim() || control().getAttribute("aria-label") || "";
  unmount();
  return name;
}

describe("DSK002-UI AutostartOffer", () => {
  it("DSK002-UI: offered shows the enable action and registered shows the stop action, each with a text of its own", async () => {
    for (const lang of ["en", "es"] as const) {
      const offered = await nameOf("offered", lang);
      const registered = await nameOf("registered", lang);
      expect(offered.length, `DSK002-UI: offered has no action text in ${lang}`).toBeGreaterThan(0);
      expect(registered.length, `DSK002-UI: registered has no action text in ${lang}`).toBeGreaterThan(0);
      expect(offered, `DSK002-UI: offered and registered must not share the action text in ${lang}`).not.toBe(registered);
    }
    expect(await nameOf("offered", "es"), "DSK002-UI: the action text is translated").not.toBe(await nameOf("offered", "en"));
  });

  it("DSK002-UI: pressing the action of offered calls onChange(true)", () => {
    const onChange = vi.fn();
    render(<AutostartOffer offer="offered" onChange={onChange} />);
    fireEvent.click(control());
    expect(onChange, "DSK002-UI: offered must call onChange(true) once").toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenLastCalledWith(true);
  });

  it("DSK002-UI: pressing the action of registered calls onChange(false)", () => {
    const onChange = vi.fn();
    render(<AutostartOffer offer="registered" onChange={onChange} />);
    fireEvent.click(control());
    expect(onChange, "DSK002-UI: registered must call onChange(false) once").toHaveBeenCalledTimes(1);
    expect(onChange).toHaveBeenLastCalledWith(false);
  });

  it.each(["offered", "registered"] as const)("DSK002-UI: with pending the control is aria-disabled and %s does not call onChange", (offer) => {
    const onChange = vi.fn();
    render(<AutostartOffer offer={offer} pending onChange={onChange} />);
    expect(control(), "DSK002-UI: pending must set aria-disabled").toHaveAttribute("aria-disabled", "true");
    fireEvent.click(control());
    expect(onChange, "DSK002-UI: a pending control must not call onChange").not.toHaveBeenCalled();
  });

  it.each([
    ["en", "dark"],
    ["en", "light"],
    ["es", "dark"],
    ["es", "light"],
  ] as const)("DSK002-UI: no axe violations in %s / %s", async (lang, theme) => {
    await setLang(lang);
    for (const offer of ["offered", "registered"] as const) {
      for (const pending of [false, true]) {
        const { container, unmount } = render(
          <div data-theme={theme}>
            <AutostartOffer offer={offer} pending={pending} onChange={() => {}} />
          </div>,
        );
        const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
        expect(results.violations.map((v) => `${offer}/${pending}: ${v.id}: ${v.help}`)).toEqual([]);
        unmount();
      }
    }
  });
});

describe("DSK002-UI error catalog", () => {
  const camel = (code: string) => code.replace(/-([a-z])/g, (_, c: string) => c.toUpperCase());
  /** DS T007 names the key `error.<code>` (kebab); the catalog test FE3 wants lowerCamel segments: either is accepted. */
  const lookup = (catalog: unknown, code: string): unknown => {
    const errors = (catalog as { error?: Record<string, unknown> }).error ?? {};
    return errors[code] ?? errors[camel(code)];
  };

  it.each(["not-offered", "cancelled", "failed"])("DSK002-UI: the error %s has a text in en.json and es.json", (code) => {
    for (const [lang, catalog] of [["en", en], ["es", es]] as const) {
      const text = lookup(catalog, code);
      expect(typeof text === "string" && text.trim().length > 0, `DSK002-UI: ${lang}.json has no text for the error ${code}`).toBe(true);
    }
  });
});
