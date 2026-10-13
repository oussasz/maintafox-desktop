import { i18n } from "@/i18n";

const SUPPORTED = new Set(["fr", "en"]);

/** Apply Settings `locale.fallback_language` to i18next (fr | en only). */
export function applyI18nFallbackLanguage(code: string): void {
  const normalized = code.trim().toLowerCase();
  const fallback = SUPPORTED.has(normalized) ? normalized : "en";
  i18n.options.fallbackLng = fallback;
}
