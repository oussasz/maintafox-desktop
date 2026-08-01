/**
 * useMediaQuery — subscribe to a CSS media query.
 * Defaults to `true` when matchMedia is unavailable (SSR / jsdom) so desktop
 * layouts remain the test and fallback path.
 */

import { useEffect, useState } from "react";

export function useMediaQuery(query: string, defaultMatches = true): boolean {
  const [matches, setMatches] = useState(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return defaultMatches;
    }
    return window.matchMedia(query).matches;
  });

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }
    const mql = window.matchMedia(query);
    const onChange = () => setMatches(mql.matches);
    onChange();
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, [query]);

  return matches;
}
