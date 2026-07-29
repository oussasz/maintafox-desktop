/**
 * Thin adapters from the identification dialog onto the official export system.
 * Prefer importing from `@/export` for new callers.
 * @see docs/UX_EXPORT_SYSTEM.md
 */

import { logoColorPng } from "@/assets/logo";
import type { IdentificationLabelContext } from "@/components/identification/identification-types";
import {
  buildQrLabelHtml,
  exportDocument,
  qrLabelPngFilename,
  renderQrLabelPngBlob,
  type ExportResult,
  type QrLabelModel,
} from "@/export";

export function toQrLabelModel(
  context: IdentificationLabelContext,
  companyName?: string | null,
): QrLabelModel {
  return {
    logoSrc: logoColorPng,
    companyName: companyName ?? null,
    qrPayload: context.payload,
    primaryCode: context.primaryCode,
    name: context.name,
    metadata: context.metadata ?? [],
    filenameStem: context.filenameStem,
  };
}

/** Save a full identity label PNG via native Save dialog. */
export async function downloadIdentificationQrPng(
  context: IdentificationLabelContext,
  companyName?: string | null,
): Promise<ExportResult> {
  const model = toQrLabelModel(context, companyName);
  return exportDocument({
    format: "png",
    image: {
      filename: qrLabelPngFilename(model.filenameStem),
      saveDialogTitle: context.dialogTitle,
      render: () => renderQrLabelPngBlob(model),
    },
  });
}

export interface IdentificationPrintOptions {
  context: IdentificationLabelContext;
  svgHtml: string;
  companyName?: string | null | undefined;
  templateId?: "label-thermal" | "label-a4";
}

/** Print a dedicated label document in a temporary OS window. */
export async function printIdentificationLabel(
  options: IdentificationPrintOptions,
): Promise<ExportResult> {
  const model = toQrLabelModel(options.context, options.companyName);
  const html = buildQrLabelHtml(model, options.svgHtml, options.templateId ?? "label-thermal");
  return exportDocument({
    format: "print",
    document: {
      title: `QR — ${model.primaryCode}`,
      html,
      templateId: options.templateId ?? "label-thermal",
    },
  });
}
