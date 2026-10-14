import type { TFunction } from "i18next";

import type { ReportTemplate } from "@shared/ipc-types";

/**
 * Translated title of a report template, keyed by its stable `code` (the DB title is the
 * English seed text). A template added later without translation keeps its stored title.
 */
export function reportTemplateTitle(
  t: TFunction<"reports">,
  tpl: Pick<ReportTemplate, "code" | "title">,
): string {
  return t(`templates.catalog.${tpl.code}.title`, { defaultValue: tpl.title });
}

export function reportTemplateDescription(
  t: TFunction<"reports">,
  tpl: Pick<ReportTemplate, "code" | "description">,
): string {
  return t(`templates.catalog.${tpl.code}.description`, { defaultValue: tpl.description });
}

/** "PDF" / "Excel (XLSX)" — export formats the backend accepts. */
export function reportFormatLabel(t: TFunction<"reports">, code: string): string {
  return t(`format.${code.toLowerCase()}`, { defaultValue: code.toUpperCase() });
}
