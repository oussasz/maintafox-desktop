/**
 * Decimal entry: what a user types in a quantity, amount or duration field, and how a stored
 * number is shown back in it.
 *
 * Typing accepts the separator of either language ("2,5" and "2.5"), spaces as thousands
 * separators ("1 250,5") and no exponent. Showing uses the app language, without thousands
 * grouping (a grouped value is harder to edit): "1250,5" in French, "1250.5" in English.
 */

/** What `parseDecimalInput` made of the text: a number, nothing (empty), or not a number. */
export type ParsedDecimal =
  | { kind: "empty" }
  | { kind: "number"; value: number }
  | { kind: "invalid" };

export interface ParseDecimalOptions {
  /** Accept a leading minus sign (default false: quantities and amounts are not negative). */
  allowNegative?: boolean;
}

const SPACES = /\s/g; // also covers the no-break spaces French uses as thousands separator
// One separator at most, digits around it (".5" and "5." are accepted).
const UNSIGNED = /^(\d+[.,]?\d*|[.,]\d+)$/;
const SIGNED = /^-?(\d+[.,]?\d*|[.,]\d+)$/;

export function parseDecimalInput(raw: string, options: ParseDecimalOptions = {}): ParsedDecimal {
  const text = raw.replace(SPACES, "");
  if (text === "") return { kind: "empty" };
  if (!(options.allowNegative ? SIGNED : UNSIGNED).test(text)) return { kind: "invalid" };
  const value = Number(text.replace(",", "."));
  return Number.isFinite(value) ? { kind: "number", value } : { kind: "invalid" };
}

const INTL_LOCALE: Record<string, string> = { fr: "fr-FR", en: "en-US", ar: "ar-DZ" };

export interface FormatDecimalInputOptions {
  /** Most decimal places kept (default 2). */
  decimals?: number;
  /** Always show exactly `decimals` places (amounts); default trims trailing zeros (quantities). */
  fixed?: boolean;
}

/** A stored number as the text of an entry field: app-language separator, no grouping, no exponent. */
export function formatDecimalInput(
  value: number | null | undefined,
  locale: string,
  options: FormatDecimalInputOptions = {},
): string {
  if (value == null || !Number.isFinite(value)) return "";
  const decimals = options.decimals ?? 2;
  return new Intl.NumberFormat(INTL_LOCALE[locale] ?? "fr-FR", {
    useGrouping: false,
    minimumFractionDigits: options.fixed ? decimals : 0,
    maximumFractionDigits: decimals,
  }).format(value);
}
