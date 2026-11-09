import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { type Language, useUiStore } from "@/state/ui-store";
import en from "./locales/en.json";
import es from "./locales/es.json";

/** `auto` follows the first system language: Spanish if it starts with `es`, English otherwise. */
export function resolveLanguage(preference: Language): "en" | "es" {
  if (preference !== "auto") return preference;
  return navigator.languages[0]?.toLowerCase().startsWith("es") ? "es" : "en";
}

i18n.on("languageChanged", (lng) => {
  document.documentElement.lang = lng;
});

/** Single i18next instance, initialised synchronously on import (resources are in the bundle). */
void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, es: { translation: es } },
  lng: resolveLanguage(useUiStore.getState().language),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
  initAsync: false,
});

useUiStore.subscribe((state, previous) => {
  if (state.language !== previous.language) void i18n.changeLanguage(resolveLanguage(state.language));
});

export default i18n;
