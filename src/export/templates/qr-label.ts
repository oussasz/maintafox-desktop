/**
 * QR / identity label template — shared model for on-screen, PNG, and print.
 */

import QRCode from "qrcode";

import { logoColorPng } from "@/assets/logo";
import { canvasToPngBlob, loadImage } from "@/export/image-exporter";
import type { PrintTemplateId } from "@/export/types";

export interface QrLabelMetadataRow {
  label: string;
  value: string;
}

export interface QrLabelModel {
  logoSrc?: string;
  companyName?: string | null;
  qrPayload: string;
  primaryCode: string;
  name: string;
  metadata: QrLabelMetadataRow[];
  filenameStem: string;
}

const DISPLAY_QR_PX = 280;
const PNG_SCALE = 3;
const PNG_QR_PX = DISPLAY_QR_PX * PNG_SCALE;

function escapeHtml(value: string): string {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function pageCss(templateId: PrintTemplateId): string {
  if (templateId === "label-a4" || templateId === "document-a4") {
    return `
      @page { size: A4; margin: 18mm; }
      body { padding: 12mm; }
    `;
  }
  // label-thermal — compact centered label
  return `
    @page { margin: 4mm; }
    body { padding: 8mm; }
  `;
}

/**
 * Dedicated printable HTML for a QR identity label (no UI chrome).
 */
export function buildQrLabelHtml(
  model: QrLabelModel,
  svgHtml: string,
  templateId: PrintTemplateId = "label-thermal",
): string {
  const logoSrc = model.logoSrc ?? logoColorPng;
  const company = model.companyName?.trim() ?? "";
  const metadataHtml = model.metadata
    .map(
      (row) =>
        `<div class="meta"><span class="meta-label">${escapeHtml(row.label)}</span> ${escapeHtml(row.value)}</div>`,
    )
    .join("");

  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8" />
  <title>QR — ${escapeHtml(model.primaryCode)}</title>
  <style>
    ${pageCss(templateId)}
    * { box-sizing: border-box; }
    body {
      margin: 0;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
      font-family: system-ui, -apple-system, Segoe UI, sans-serif;
      color: #111;
      background: #fff;
      text-align: center;
    }
    .logo { width: 48px; height: 48px; object-fit: contain; margin-bottom: 6px; }
    .company { font-size: 12px; color: #4b5563; margin-bottom: 16px; }
    .qr { display: flex; align-items: center; justify-content: center; margin: 0; padding: 0; border: none; background: transparent; }
    .qr svg { width: ${DISPLAY_QR_PX}px; height: ${DISPLAY_QR_PX}px; display: block; }
    .code {
      margin-top: 16px;
      font-size: 18px;
      font-weight: 700;
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    }
    .name { margin-top: 6px; font-size: 14px; color: #374151; }
    .meta { margin-top: 4px; font-size: 12px; color: #6b7280; }
    .meta-label { font-weight: 600; color: #4b5563; }
  </style>
</head>
<body>
  <img class="logo" src="${escapeHtml(logoSrc)}" alt="" />
  ${company ? `<div class="company">${escapeHtml(company)}</div>` : ""}
  <div class="qr">${svgHtml}</div>
  <div class="code">${escapeHtml(model.primaryCode)}</div>
  <div class="name">${escapeHtml(model.name)}</div>
  ${metadataHtml}
</body>
</html>`;
}

/**
 * Compose a high-resolution PNG that matches the on-screen label hierarchy.
 */
export async function renderQrLabelPngBlob(model: QrLabelModel): Promise<Blob> {
  const pad = 48 * PNG_SCALE;
  const gap = 12 * PNG_SCALE;
  const logoSize = 48 * PNG_SCALE;
  const width = Math.max(PNG_QR_PX + pad * 2, 420 * PNG_SCALE);

  // Measure vertical stack
  let height = pad;
  height += logoSize + gap;
  if (model.companyName?.trim()) height += 18 * PNG_SCALE + gap;
  height += PNG_QR_PX + gap * 2;
  height += 28 * PNG_SCALE + gap; // code
  height += 22 * PNG_SCALE + gap; // name
  height += model.metadata.length * (18 * PNG_SCALE + 4 * PNG_SCALE);
  height += pad;

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("QrLabel: 2D canvas context unavailable.");

  // White background
  ctx.fillStyle = "#ffffff";
  ctx.fillRect(0, 0, width, height);

  let y = pad;
  const cx = width / 2;

  // Logo
  try {
    const logo = await loadImage(model.logoSrc ?? logoColorPng);
    ctx.drawImage(logo, cx - logoSize / 2, y, logoSize, logoSize);
  } catch {
    // Logo optional if asset fails to load
  }
  y += logoSize + gap;

  // Company
  const company = model.companyName?.trim();
  if (company) {
    ctx.fillStyle = "#4b5563";
    ctx.font = `${14 * PNG_SCALE}px system-ui, sans-serif`;
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    ctx.fillText(company, cx, y);
    y += 18 * PNG_SCALE + gap;
  }

  // QR
  const qrCanvas = document.createElement("canvas");
  await QRCode.toCanvas(qrCanvas, model.qrPayload, {
    width: PNG_QR_PX,
    margin: 1,
    errorCorrectionLevel: "M",
    color: { dark: "#000000", light: "#ffffff" },
  });
  ctx.drawImage(qrCanvas, cx - PNG_QR_PX / 2, y);
  y += PNG_QR_PX + gap * 2;

  // Primary code
  ctx.fillStyle = "#111111";
  ctx.font = `bold ${18 * PNG_SCALE}px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace`;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  ctx.fillText(model.primaryCode, cx, y);
  y += 28 * PNG_SCALE + gap;

  // Name
  ctx.fillStyle = "#374151";
  ctx.font = `${14 * PNG_SCALE}px system-ui, sans-serif`;
  ctx.fillText(model.name, cx, y, width - pad * 2);
  y += 22 * PNG_SCALE + gap;

  // Metadata
  ctx.fillStyle = "#6b7280";
  ctx.font = `${12 * PNG_SCALE}px system-ui, sans-serif`;
  for (const row of model.metadata) {
    ctx.fillText(`${row.label}: ${row.value}`, cx, y, width - pad * 2);
    y += 18 * PNG_SCALE + 4 * PNG_SCALE;
  }

  return canvasToPngBlob(canvas);
}

export function qrLabelPngFilename(stem: string): string {
  const safe = stem.replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_").trim() || "Asset";
  return `Asset_${safe}_QR.png`;
}
