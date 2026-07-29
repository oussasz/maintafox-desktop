import type { ReactNode } from "react";

export interface IdentificationLabelMetadataRow {
  label: string;
  value: string;
}

/** Entity-agnostic label content for the enterprise identification dialog. */
export interface IdentificationLabelContext {
  dialogTitle: string;
  /** Human-readable primary identifier shown on the label (e.g. asset code). */
  primaryCode: string;
  /** Entity name / subtitle. */
  name: string;
  /** Optional key/value rows (class, site, WO number, etc.). */
  metadata?: IdentificationLabelMetadataRow[];
  /** Encoded QR payload (consumer-owned). */
  payload: string;
  /** PNG filename stem (without extension). */
  filenameStem: string;
  /** Optional short description under the title. */
  description?: string;
}

export interface IdentificationBarcodeSlot {
  /** Reserved for future barcode rendering. Value only for now. */
  value: string;
}

export interface IdentificationLabelDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  context: IdentificationLabelContext;
  /** Show product / company branding above the QR. Default true. */
  showLogo?: boolean;
  /** Optional logo override (future tenant branding). */
  logo?: ReactNode;
  /** Optional company caption under the logo. */
  companyName?: string | null;
  /** Reserved barcode slot — rendered only when provided. */
  barcode?: IdentificationBarcodeSlot | null;
  /** Localized labels for actions / a11y. */
  labels: {
    downloadPng: string;
    printLabel: string;
    close: string;
    companyLabel?: string;
  };
}
