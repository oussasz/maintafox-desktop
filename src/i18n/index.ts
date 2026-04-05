// src/i18n/index.ts
// Entry point for i18n. Call initI18n() before rendering the React tree.
import "./types";
export { i18n, initI18n, DEFAULT_LOCALE, FALLBACK_LOCALE } from "./config";
export { ALL_NAMESPACES, SUPPORTED_LOCALES, LOCALE_DISPLAY_NAMES } from "./namespaces";
export type { AppNamespace, SupportedLocale, EagerNamespace, ModuleNamespace } from "./namespaces";
