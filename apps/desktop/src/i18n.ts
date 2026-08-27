import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "../../../locales/en.json";
import ko from "../../../locales/ko.json";

export const resources = {
  en: { translation: en },
  ko: { translation: ko },
} as const;

export type SupportedLocale = keyof typeof resources;

function initialLocale(): SupportedLocale {
  return typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("ko")
    ? "ko"
    : "en";
}

void i18n.use(initReactI18next).init({
  resources,
  lng: initialLocale(),
  fallbackLng: "en",
  supportedLngs: ["en", "ko"],
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
