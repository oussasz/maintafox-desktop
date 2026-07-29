/**
 * PdfExporter — stub until Rust/reports adapter lands.
 */

import { ExportNotImplementedError } from "@/export/types";

export async function exportPdf(_filename: string, _render: () => Promise<Blob>): Promise<void> {
  throw new ExportNotImplementedError("pdf");
}
