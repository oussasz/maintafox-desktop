/**
 * ExportActions — unified export footer for Maintafox dialogs.
 * Close (outline) · Print (outline secondary) · Download PNG (primary).
 */

import { Download, Loader2, Printer } from "lucide-react";

import { Button } from "@/components/ui/button";
import { mfExport } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface ExportActionsLabels {
  close: string;
  print: string;
  downloadPng: string;
}

export interface ExportActionsProps {
  onClose: () => void;
  onPrint: () => void | Promise<void>;
  onDownloadPng: () => void | Promise<void>;
  labels: ExportActionsLabels;
  busy?: boolean;
  disabled?: boolean;
  className?: string;
}

export function ExportActions({
  onClose,
  onPrint,
  onDownloadPng,
  labels,
  busy = false,
  disabled = false,
  className,
}: ExportActionsProps) {
  const locked = busy || disabled;

  return (
    <div className={cn(mfExport.actions, className)}>
      <Button type="button" variant="outline" onClick={onClose} disabled={busy}>
        {labels.close}
      </Button>
      <Button
        type="button"
        variant="outline"
        disabled={locked}
        onClick={() => void onPrint()}
      >
        <Printer className="mr-1.5 h-3.5 w-3.5" />
        {labels.print}
      </Button>
      <Button type="button" disabled={locked} onClick={() => void onDownloadPng()}>
        {busy ? (
          <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
        ) : (
          <Download className="mr-1.5 h-3.5 w-3.5" />
        )}
        {labels.downloadPng}
      </Button>
    </div>
  );
}
