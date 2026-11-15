// Light / dark appearance: follows the phone unless the technician pins one in Settings.
// Stored on this phone only (a display preference, not plant data).

export const APPEARANCES = ["system", "light", "dark"] as const;
export type Appearance = (typeof APPEARANCES)[number];

export const APPEARANCE_STORAGE_KEY = "maintafox-mobile:appearance";

function isAppearance(value: unknown): value is Appearance {
  return typeof value === "string" && (APPEARANCES as readonly string[]).includes(value);
}

export function storedAppearance(): Appearance {
  try {
    const value = localStorage.getItem(APPEARANCE_STORAGE_KEY);
    return isAppearance(value) ? value : "system";
  } catch {
    return "system";
  }
}

/** Sets `data-theme` on <html> (none for "system", so the CSS follows prefers-color-scheme). */
export function applyAppearance(appearance: Appearance): void {
  if (appearance === "system") delete document.documentElement.dataset["theme"];
  else document.documentElement.dataset["theme"] = appearance;
}

export function setAppearance(appearance: Appearance): void {
  applyAppearance(appearance);
  try {
    localStorage.setItem(APPEARANCE_STORAGE_KEY, appearance);
  } catch {
    // Storage unavailable: the choice lasts for this run only.
  }
}
