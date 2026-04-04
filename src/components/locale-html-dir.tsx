/**
 * locale-html-dir.tsx
 *
 * Sets the `dir` attribute on <html> whenever the active locale changes.
 * Renders nothing — mount this once at the root of the application tree.
 *
 * Usage in App.tsx:
 *   <LocaleHtmlDir />     (before any other providers)
 *
 * When Arabic is added (Phase 3), this component will set dir="rtl"
 * automatically without any further changes required.
 */

import { useEffect } from "react";

import { useLocaleStore } from "@/stores/locale-store";

export function LocaleHtmlDir(): null {
  const direction = useLocaleStore((s) => s.direction);
  const activeLocale = useLocaleStore((s) => s.activeLocale);

  useEffect(() => {
    document.documentElement.setAttribute("dir", direction);
    document.documentElement.setAttribute("lang", activeLocale);
  }, [direction, activeLocale]);

  return null;
}
