/**
 * locale-html-dir.tsx
 *
 * Sets the `dir` attribute on <html> whenever the active locale changes.
 * Also hydrates locale + currency preferences once at app root.
 * Renders nothing — mount this once at the root of the application tree.
 */

import { useEffect } from "react";

import { useLocaleStore } from "@/stores/locale-store";

export function LocaleHtmlDir(): null {
  const direction = useLocaleStore((s) => s.direction);
  const activeLocale = useLocaleStore((s) => s.activeLocale);
  const initialize = useLocaleStore((s) => s.initialize);

  useEffect(() => {
    void initialize();
  }, [initialize]);

  useEffect(() => {
    document.documentElement.setAttribute("dir", direction);
    document.documentElement.setAttribute("lang", activeLocale);
  }, [direction, activeLocale]);

  return null;
}
