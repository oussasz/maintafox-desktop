/**
 * CSV export helpers shared by every "Exporter (Excel/CSV)" button and every
 * import template.
 *
 * Files use ";" as separator and a UTF-8 byte-order mark so a French Excel
 * opens them with the right columns and accents on a double click.
 */

import { saveBlobWithDialog } from "@/export/download";

export const CSV_SEPARATOR = ";";

export type CsvCell = string | number | boolean | null | undefined;

// UTF-8 byte-order mark: makes Excel read accents correctly.
const BOM = String.fromCharCode(0xfeff);

function escapeCell(value: CsvCell): string {
  if (value === null || value === undefined) return "";
  const text = String(value);
  // Neutralise spreadsheet formulas typed into free-text fields (text only: numbers stay numbers).
  const safe = typeof value === "string" && /^[=+\-@\t\r]/.test(text) ? `'${text}` : text;
  return /[";\r\n]/.test(safe) ? `"${safe.replace(/"/g, '""')}"` : safe;
}

/** Builds the CSV text (without BOM) from a header line and data rows. */
export function buildCsv(header: string[], rows: CsvCell[][]): string {
  return [header, ...rows].map((line) => line.map(escapeCell).join(CSV_SEPARATOR)).join("\r\n");
}

/** Builds the downloadable blob (BOM + CSV). */
export function csvBlob(header: string[], rows: CsvCell[][]): Blob {
  return new Blob([`${BOM}${buildCsv(header, rows)}\r\n`], { type: "text/csv;charset=utf-8" });
}

/** Opens the save dialog and writes the CSV file. Returns the saved path. */
export async function saveCsvWithDialog(
  fileName: string,
  header: string[],
  rows: CsvCell[][],
  title?: string,
): Promise<string> {
  return saveBlobWithDialog(csvBlob(header, rows), fileName, {
    ...(title ? { title } : {}),
    filters: [{ name: "CSV (Excel)", extensions: ["csv"] }],
  });
}
