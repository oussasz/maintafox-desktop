/**
 * CSV for reference values: the file a manager exports, edits in a spreadsheet and imports back.
 *
 * RFC 4180: comma separator, fields with a comma, quote or line break are quoted, quotes doubled.
 * A UTF-8 byte order mark is written so spreadsheet tools read accented labels correctly, and
 * ignored on import. Column names are the import wizard's (`code,label,description,…`).
 */

import type { ImportRowInput, RefExportRow } from "@shared/ipc-types";

/** Columns written by the export, in this order; the import accepts any order. */
export const REFERENCE_CSV_COLUMNS = [
  "code",
  "label",
  "description",
  "parent_code",
  "sort_order",
  "color_hex",
  "icon_name",
  "semantic_tag",
  "external_code",
] as const;

const BOM = "﻿";

/** Splits CSV text into records of fields (quoted fields may hold commas, quotes, line breaks). */
export function splitCsvRecords(text: string): string[][] {
  const source = text.startsWith(BOM) ? text.slice(1) : text;
  const records: string[][] = [];
  let record: string[] = [];
  let field = "";
  let inQuotes = false;

  for (let i = 0; i < source.length; i++) {
    const ch = source[i];
    if (inQuotes) {
      if (ch === '"') {
        if (source[i + 1] === '"') {
          field += '"';
          i++;
        } else {
          inQuotes = false;
        }
      } else {
        field += ch;
      }
      continue;
    }
    if (ch === '"') {
      inQuotes = true;
    } else if (ch === ",") {
      record.push(field);
      field = "";
    } else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && source[i + 1] === "\n") i++;
      record.push(field);
      records.push(record);
      record = [];
      field = "";
    } else {
      field += ch;
    }
  }
  if (field !== "" || record.length > 0) {
    record.push(field);
    records.push(record);
  }
  return records.filter((r) => r.some((f) => f.trim() !== ""));
}

/** Rows of an import file; a row needs a code or a label. */
export function parseReferenceCsv(text: string): ImportRowInput[] {
  const [header, ...body] = splitCsvRecords(text);
  if (!header || body.length === 0) return [];
  const columns = header.map((h) => h.trim().toLowerCase());
  const rows: ImportRowInput[] = [];

  for (const record of body) {
    const row: ImportRowInput = {};
    columns.forEach((column, idx) => {
      const val = (record[idx] ?? "").trim();
      if (!val) return;
      switch (column) {
        case "code":
          row.code = val;
          break;
        case "label":
          row.label = val;
          break;
        case "description":
          row.description = val;
          break;
        case "parent_code":
          row.parent_code = val;
          break;
        case "sort_order": {
          const n = Number.parseInt(val, 10);
          row.sort_order = Number.isFinite(n) ? n : null;
          break;
        }
        case "color_hex":
          row.color_hex = val;
          break;
        case "icon_name":
          row.icon_name = val;
          break;
        case "semantic_tag":
          row.semantic_tag = val;
          break;
        case "external_code":
          row.external_code = val;
          break;
        case "metadata_json":
          row.metadata_json = val;
          break;
      }
    });
    if (row.code || row.label) rows.push(row);
  }
  return rows;
}

function csvField(value: string | number | null | undefined): string {
  if (value == null) return "";
  const s = String(value);
  return /[",\r\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
}

/** Export file of a set: one line per value, parents written as codes. */
export function buildReferenceCsv(rows: readonly RefExportRow[]): string {
  const codeById = new Map(rows.map((r) => [r.value.id, r.value.code]));
  const lines = [REFERENCE_CSV_COLUMNS.join(",")];
  for (const { value } of rows) {
    const parentCode = value.parent_id != null ? (codeById.get(value.parent_id) ?? "") : "";
    lines.push(
      [
        value.code,
        value.label,
        value.description,
        parentCode,
        value.sort_order,
        value.color_hex,
        value.icon_name,
        value.semantic_tag,
        value.external_code,
      ]
        .map(csvField)
        .join(","),
    );
  }
  return `${BOM}${lines.join("\r\n")}\r\n`;
}
