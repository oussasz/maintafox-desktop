/**
 * ExportManager — single entry point for PNG / print / PDF.
 */

import { exportImage } from "@/export/image-exporter";
import { exportPdf } from "@/export/pdf-exporter";
import { printHtml } from "@/export/print-manager";
import { ExportNotImplementedError, type ExportRequest, type ExportResult } from "@/export/types";

export async function exportDocument(request: ExportRequest): Promise<ExportResult> {
  switch (request.format) {
    case "png": {
      if (!request.image) {
        throw new Error("ExportManager: PNG export requires an image spec.");
      }
      const path = await exportImage(request.image);
      return { status: "saved", path, format: "png" };
    }
    case "print": {
      if (!request.document) {
        throw new Error("ExportManager: print export requires a printable document.");
      }
      await printHtml(request.document.html, { title: request.document.title });
      return { status: "printed", format: "print" };
    }
    case "pdf": {
      if (!request.pdf) {
        throw new ExportNotImplementedError("pdf");
      }
      await exportPdf(request.pdf.filename, request.pdf.render);
      return { status: "saved", path: request.pdf.filename, format: "pdf" };
    }
    default: {
      const _exhaustive: never = request.format;
      throw new Error(`ExportManager: unknown format ${_exhaustive}`);
    }
  }
}
