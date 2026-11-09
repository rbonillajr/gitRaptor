import { useState } from "react";
import { useTranslation } from "react-i18next";
import "@/i18n";
import { SegmentedControl, ToastProvider, Toggle, UiPortalProvider } from "@/ui";
import { CatalogGrid } from "./CatalogGrid";
import "./catalog.css";

/** Marks the catalog root; `tests/catalog-excluded.test.ts` greps the release build for it. */
export const CATALOG_SENTINEL = "gr-dev-catalog";

const LANGUAGES = [
  { value: "en", label: "en" },
  { value: "es", label: "es" },
] as const;

/** One theme panel: its own `data-theme`, its own portal container and its own toast queue. */
function ThemePanel({ theme, label }: { theme: "dark" | "light"; label: string }) {
  const [container, setContainer] = useState<HTMLElement | null>(null);
  return (
    <section data-theme={theme} aria-label={label} className="gr-cat-theme">
      <div ref={setContainer} />
      <UiPortalProvider container={container}>
        <ToastProvider>
          <CatalogGrid />
        </ToastProvider>
      </UiPortalProvider>
    </section>
  );
}

/**
 * Dev-only page (`/?catalog`): every primitive in a dark and a light panel at once (D5-D7). Labels are literal
 * English (D16); the primitives' own texts follow the language switch.
 */
export function Catalog() {
  const { i18n } = useTranslation();
  const [reduced, setReduced] = useState(false);
  const language = i18n.resolvedLanguage === "es" ? "es" : "en";

  return (
    <div data-gr-dev-catalog={CATALOG_SENTINEL} data-gr-motion={reduced ? "reduce" : undefined} className="gr-cat">
      <div className="gr-cat-bar">
        <h1>Design system catalog</h1>
        <SegmentedControl
          label="Language"
          options={LANGUAGES}
          value={language}
          onChange={(next) => {
            void i18n.changeLanguage(next);
          }}
        />
        <Toggle label="Reduced motion" checked={reduced} onCheckedChange={setReduced} />
      </div>
      <div className="gr-cat-panels">
        <ThemePanel theme="dark" label="Dark theme" />
        <ThemePanel theme="light" label="Light theme" />
      </div>
    </div>
  );
}
