import type { ReactNode } from "react";
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";

/** The look actually applied to the window. */
type Theme = "dark" | "light";
/** What the person chose: a fixed look, or follow the Windows setting. */
export type ThemePreference = Theme | "system";

/** localStorage key, also read by `public/theme-init.js` before the first paint. */
export const THEME_STORAGE_KEY = "maintafox:theme:v2";
const DARK_QUERY = "(prefers-color-scheme: dark)";

interface ThemeContextValue {
  /** Look currently applied (a "system" choice resolves to the Windows setting). */
  theme: Theme;
  /** The stored choice for this PC. */
  preference: ThemePreference;
  setPreference: (p: ThemePreference) => void;
  /** Sets a fixed look (kept for existing callers). */
  setTheme: (t: Theme) => void;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function readStoredPreference(): ThemePreference {
  try {
    const saved = localStorage.getItem(THEME_STORAGE_KEY);
    if (saved === "dark" || saved === "light" || saved === "system") return saved;
  } catch {
    // storage blocked: fall through to the default
  }
  return "system";
}

function systemPrefersDark(): boolean {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia(DARK_QUERY).matches;
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [preference, setPreferenceState] = useState<ThemePreference>(readStoredPreference);
  const [systemDark, setSystemDark] = useState<boolean>(systemPrefersDark);

  // Follow the Windows setting live while "system" is chosen.
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const mql = window.matchMedia(DARK_QUERY);
    const onChange = (event: MediaQueryListEvent) => setSystemDark(event.matches);
    setSystemDark(mql.matches);
    mql.addEventListener("change", onChange);
    return () => mql.removeEventListener("change", onChange);
  }, []);

  const theme: Theme = preference === "system" ? (systemDark ? "dark" : "light") : preference;

  useEffect(() => {
    const root = document.documentElement;
    root.classList.remove("light", "dark");
    root.classList.add(theme);
    root.style.colorScheme = theme;
  }, [theme]);

  const setPreference = useCallback((p: ThemePreference) => {
    setPreferenceState(p);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, p);
    } catch {
      // the choice still applies for this session
    }
  }, []);

  const value = useMemo<ThemeContextValue>(
    () => ({ theme, preference, setPreference, setTheme: setPreference }),
    [theme, preference, setPreference],
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used inside ThemeProvider");
  return ctx;
}
