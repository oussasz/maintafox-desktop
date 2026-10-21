/**
 * Display text of the values of a plant list (Données de référence): the name in the interface
 * language, plus the exact code when two values of the same list show the same name (e.g. a
 * legacy "Leave" kept next to "leave"). Codes are always matched exactly, never ignoring case.
 */

export interface NamedListOption {
  code: string;
  label: string;
  label_en: string | null;
}

/** Name of a list value in the interface language. */
export function listOptionName(option: NamedListOption, language: string): string {
  return language.toLowerCase().startsWith("en") && option.label_en
    ? option.label_en
    : option.label;
}

/** Display text of each value of a list, keyed by its exact code. */
export function listOptionTexts(
  options: readonly NamedListOption[],
  language: string,
): Map<string, string> {
  const names = options.map((o) => listOptionName(o, language));
  const key = (name: string) => name.trim().toLowerCase();
  const counts = new Map<string, number>();
  for (const n of names) counts.set(key(n), (counts.get(key(n)) ?? 0) + 1);
  const out = new Map<string, string>();
  options.forEach((o, i) => {
    const name = names[i] as string;
    out.set(o.code, (counts.get(key(name)) ?? 0) > 1 ? `${name} (${o.code})` : name);
  });
  return out;
}

/** The value with exactly this code. */
export function findListOption<T extends { code: string }>(
  options: readonly T[] | undefined,
  code: string | null | undefined,
): T | null {
  if (code == null) return null;
  return options?.find((o) => o.code === code) ?? null;
}
