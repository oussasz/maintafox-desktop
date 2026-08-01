/**
 * Official Maintafox export / print type contracts.
 * @see docs/UX_EXPORT_SYSTEM.md
 */

export type ExportFormat = "png" | "pdf" | "print";

/** Printable surface templates. */
export type PrintTemplateId = "label-thermal" | "label-a4" | "document-a4";

export type ExportTarget = "download" | "print" | "clipboard" | "share";

export interface PrintableDocument {
  title: string;
  /** Full HTML document string ready for PrintManager. */
  html: string;
  templateId?: PrintTemplateId;
}

export interface ExportableImageSpec {
  /** Suggested filename including extension (e.g. Asset_EQ-001_QR.png). */
  filename: string;
  /** MIME type for the blob (default image/png). */
  mimeType?: string;
  /** Dialog title for the native Save File picker. */
  saveDialogTitle?: string;
  /** Async factory that produces the image blob (called after user confirms save). */
  render: () => Promise<Blob>;
}

export interface ExportRequest {
  format: ExportFormat;
  /** Used for print. */
  document?: PrintableDocument;
  /** Used for PNG (and future raster formats). */
  image?: ExportableImageSpec;
  /** Used when format is pdf (stub until Rust adapter). */
  pdf?: { filename: string; render: () => Promise<Blob>; saveDialogTitle?: string };
}

/** Successful or cancelled export outcome — never silent. */
export type ExportResult =
  | { status: "saved"; path: string; format: ExportFormat }
  | { status: "printed"; format: "print" }
  | { status: "cancelled"; format: ExportFormat };

export class ExportNotImplementedError extends Error {
  readonly code = "EXPORT_NOT_IMPLEMENTED" as const;

  constructor(format: ExportFormat) {
    super(`Export format "${format}" is not implemented yet.`);
    this.name = "ExportNotImplementedError";
  }
}

/** User dismissed the native Save dialog — not an error for UX. */
export class ExportCancelledError extends Error {
  readonly code = "EXPORT_CANCELLED" as const;

  constructor() {
    super("Export cancelled by user.");
    this.name = "ExportCancelledError";
  }
}

export function isExportCancelled(err: unknown): boolean {
  return (
    err instanceof ExportCancelledError ||
    (typeof err === "object" &&
      err !== null &&
      "code" in err &&
      (err as { code: unknown }).code === "EXPORT_CANCELLED")
  );
}
