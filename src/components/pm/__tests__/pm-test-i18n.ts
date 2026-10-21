/**
 * Test translator for the PM screens: `pm` keys resolve against the real English catalogue, so a
 * missing key fails the test; other namespaces echo `key` (+ `:code` when given).
 */
import enPm from "@/i18n/locale-data/en/pm.json";

function lookup(key: string): unknown {
  return key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], enPm);
}

function interpolate(value: string, opts?: Record<string, unknown>): string {
  return value.replace(/\{\{(\w+)\}\}/g, (_, name: string) => String(opts?.[name] ?? ""));
}

export function translatePm(key: string, opts?: Record<string, unknown>): string {
  let value = lookup(key);
  if (typeof value !== "string" && typeof opts?.["count"] === "number") {
    value = lookup(`${key}_${opts["count"] === 1 ? "one" : "other"}`);
  }
  if (typeof value !== "string") {
    if (typeof opts?.["defaultValue"] === "string") return opts["defaultValue"];
    throw new Error(`missing pm i18n key: ${key}`);
  }
  return interpolate(value, opts);
}

export function translateOther(key: string, opts?: Record<string, unknown>): string {
  return opts?.["code"] != null ? `${key}:${String(opts["code"])}` : key;
}

export const pmI18nMock = {
  useTranslation: (ns?: string) => ({
    t: ns === "pm" ? translatePm : translateOther,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
};
