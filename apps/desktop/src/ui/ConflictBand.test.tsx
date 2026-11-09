// D8-BAND (DS-US-DSK-008 D6, D19, § 9.4): the presentation piece of a predicted conflict. Row and badge variants;
// level, staleness and "new" in text (WCAG 1.4.1); the glyph hidden from assistive technology; each "View" names its
// pair; a band without detail shows the pair without a file.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import axe from "axe-core";
import i18n from "i18next";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "@/i18n";
import { ConflictBand, type ConflictBandProps } from "./ConflictBand";

const setLang = (l: "en" | "es") => act(async () => void (await i18n.changeLanguage(l)));
const A = "Claude Code · feat-pagos";
const B = "Claude Code · feat-login";
const props = (over: Partial<ConflictBandProps> = {}): ConflictBandProps => ({
  variant: "row",
  level: "conflict",
  aLabel: A,
  bLabel: B,
  firstFile: "pagos.rs",
  moreText: "and 2 more",
  freshnessText: "40 s ago",
  stale: false,
  isNew: false,
  a11yLabel: `predicted conflict between ${A} and ${B}, pagos.rs, 40 s ago`,
  onOpen: () => {},
  ...over,
});

beforeEach(() => setLang("en"));
afterEach(cleanup);

describe("ConflictBand", () => {
  it("level, staleness and new are said in text", () => {
    const { container, rerender } = render(<ConflictBand {...props()} />);
    expect(container.textContent, "the level in words").toMatch(/predicted conflict/i);
    expect(container.textContent).toContain(A);
    expect(container.textContent).toContain(B);
    expect(screen.getByText("pagos.rs").tagName, "the first file is code").toBe("CODE");
    expect(container.textContent).toContain("and 2 more");
    expect(container.textContent).toContain("40 s ago");
    expect(screen.queryByText("new"), "not new: no mark").toBeNull();
    rerender(<ConflictBand {...props({ isNew: true, stale: true, freshnessText: "outdated · 40 s ago · recalculating" })} />);
    expect(screen.getByText("new"), "new is a word, not only a color").toBeInTheDocument();
    expect(container.textContent, "stale is a word").toContain("outdated · 40 s ago · recalculating");
    rerender(<ConflictBand {...props({ level: "overlap", a11yLabel: "x" })} />);
    expect(container.textContent, "a warning says what it is").toMatch(/same files, not committed yet/i);
  });

  it("the accessible name reads the pair with and", () => {
    render(<ConflictBand {...props()} />);
    const named = screen.getByLabelText(`predicted conflict between ${A} and ${B}, pagos.rs, 40 s ago`);
    expect(named, "the band is named by a11yLabel, with `and` and no arrow").toBeInTheDocument();
    expect(named.getAttribute("aria-label")).not.toContain("↔");
    expect(named.getAttribute("aria-label")).not.toContain("⚡");
  });

  it("the glyph is hidden from assistive tech and View names its pair", async () => {
    const { container } = render(<ConflictBand {...props()} />);
    const hidden = [...container.querySelectorAll('[aria-hidden="true"]')].map((n) => n.textContent);
    expect(hidden, "the ⚡ glyph is inside an aria-hidden span").toContain("⚡");
    const open = screen.getByRole("button", { name: `View the conflict between ${A} and ${B}` });
    expect(open.textContent, "the visible text is View, which the accessible name starts with (WCAG 2.5.3)").toBe("View");
    expect(open.getAttribute("aria-label")?.startsWith("View")).toBe(true);
    await setLang("es");
    expect(screen.getByRole("button", { name: `Ver el conflicto entre ${A} y ${B}` }).textContent).toBe("Ver");
  });

  it("View calls onOpen and the button takes the openRef", () => {
    const onOpen = vi.fn();
    const openRef = createRef<HTMLButtonElement>();
    render(<ConflictBand {...props({ onOpen, openRef })} />);
    fireEvent.click(screen.getByRole("button", { name: /^View the conflict/ }));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(openRef.current, "the ref points at the View button").toBe(screen.getByRole("button", { name: /^View the conflict/ }));
  });

  it("the badge variant has no button and the same text", () => {
    const { container } = render(<ConflictBand {...props({ variant: "badge" })} />);
    expect(screen.queryByRole("button"), "the lane activates the badge, it has no button").toBeNull();
    expect(container.textContent).toContain("pagos.rs");
    expect(container.textContent).toContain("40 s ago");
    expect([...container.querySelectorAll('[aria-hidden="true"]')].map((n) => n.textContent)).toContain("⚡");
  });

  it("a band without detail shows the pair without a file", () => {
    const { container } = render(<ConflictBand {...props({ firstFile: null, moreText: null, a11yLabel: "x" })} />);
    expect(container.textContent).toContain(A);
    expect(container.textContent).toContain(B);
    expect(container.querySelector("code"), "no file, no code").toBeNull();
    expect(container.textContent).not.toContain("pagos.rs");
  });

  it.each(["en", "es"] as const)("has no axe violations in %s, row and badge", async (lang) => {
    await setLang(lang);
    for (const variant of ["row", "badge"] as const) {
      const { container, unmount } = render(
        <ul>
          <li>
            <ConflictBand {...props({ variant, isNew: true })} />
          </li>
        </ul>,
      );
      expect(container.textContent, `the ${variant} paints the pair before axe looks at it`).toContain(A);
      const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
      expect(results.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
      unmount();
    }
  });
});
