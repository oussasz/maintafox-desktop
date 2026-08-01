/**
 * IdentificationLabelDialog — official Maintafox enterprise QR / identity label modal.
 * Portaled Radix Dialog; borderless industrial label; export via `@/export`.
 * @see docs/UX_IDENTIFICATION_DIALOG_PATTERN.md
 * @see docs/UX_EXPORT_SYSTEM.md
 */

import { Loader2 } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { MaintafoxWordmark } from "@/components/branding/MaintafoxWordmark";
import {
  downloadIdentificationQrPng,
  printIdentificationLabel,
} from "@/components/identification/identification-label-export";
import type { IdentificationLabelDialogProps } from "@/components/identification/identification-types";
import { useIdentificationQr } from "@/components/identification/useIdentificationQr";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Toast,
  ToastAction,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
} from "@/components/ui/toast";
import { mfIdentification } from "@/design-system/tokens";
import { ExportActions, isExportCancelled, revealInFolder } from "@/export";
import { useToast } from "@/hooks/use-toast";
import { cn } from "@/lib/utils";
import { toErrorMessage } from "@/utils/errors";

export function IdentificationLabelDialog({
  open,
  onOpenChange,
  context,
  showLogo = true,
  logo,
  companyName,
  barcode = null,
  labels,
}: IdentificationLabelDialogProps) {
  const { t } = useTranslation("equipment");
  const { toasts, toast, dismiss } = useToast();
  const { svgHtml, loading, error } = useIdentificationQr(context.payload, open);
  const [exportBusy, setExportBusy] = useState(false);
  const [exportError, setExportError] = useState<string | null>(null);

  const canExport = !loading && !error && Boolean(svgHtml);

  const handleDownload = useCallback(async () => {
    setExportError(null);
    setExportBusy(true);
    try {
      const result = await downloadIdentificationQrPng(context, companyName);
      if (result.status === "saved") {
        toast({
          title: t("qr.export.savedTitle"),
          description: result.path,
          variant: "success",
        });
      }
    } catch (err) {
      if (isExportCancelled(err)) {
        return;
      }
      const msg = toErrorMessage(err);
      setExportError(msg);
      toast({ title: t("qr.export.failedTitle"), description: msg, variant: "destructive" });
    } finally {
      setExportBusy(false);
    }
  }, [companyName, context, t, toast]);

  const handlePrint = useCallback(async () => {
    if (!svgHtml) return;
    setExportError(null);
    setExportBusy(true);
    try {
      await printIdentificationLabel({
        context,
        svgHtml,
        companyName: companyName ?? null,
      });
      toast({
        title: t("qr.export.printStartedTitle"),
        description: t("qr.export.printStartedDescription"),
        variant: "default",
      });
    } catch (err) {
      const msg = toErrorMessage(err);
      setExportError(msg);
      toast({ title: t("qr.export.failedTitle"), description: msg, variant: "destructive" });
    } finally {
      setExportBusy(false);
    }
  }, [companyName, context, svgHtml, t, toast]);

  const handleOpenFolder = useCallback(
    async (path: string) => {
      try {
        await revealInFolder(path);
      } catch (err) {
        toast({
          title: t("qr.export.failedTitle"),
          description: toErrorMessage(err),
          variant: "destructive",
        });
      }
    },
    [t, toast],
  );

  const barcodeEnabled = Boolean(barcode?.value);
  const companyCaption = useMemo(() => companyName?.trim() || null, [companyName]);

  return (
    <ToastProvider>
      <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className={cn(mfIdentification.dialog)}>
          <DialogHeader className={mfIdentification.header}>
            <DialogTitle className={mfIdentification.title}>{context.dialogTitle}</DialogTitle>
            {context.description ? (
              <DialogDescription className={mfIdentification.description}>
                {context.description}
              </DialogDescription>
            ) : null}
          </DialogHeader>

          <div className={mfIdentification.body}>
            <div className={mfIdentification.labelStack}>
              {showLogo && (
                <div className={mfIdentification.logoRow}>
                  {logo ?? <MaintafoxWordmark size="md" showText={false} align="center" />}
                  {companyCaption ? (
                    <p className="text-xs text-text-muted">
                      {labels.companyLabel ? `${labels.companyLabel}: ` : null}
                      {companyCaption}
                    </p>
                  ) : null}
                </div>
              )}

              {loading ? (
                <div className={cn(mfIdentification.qrSize, "text-text-muted")}>
                  <Loader2 className="h-8 w-8 animate-spin" aria-hidden />
                </div>
              ) : error ? (
                <div
                  className={cn(
                    mfIdentification.qrSize,
                    "px-4 text-center text-sm text-status-danger",
                  )}
                >
                  {error}
                </div>
              ) : (
                <div
                  className={mfIdentification.qrSize}
                  // biome-ignore lint/security/noDangerouslySetInnerHtml: SVG generated by qrcode lib
                  dangerouslySetInnerHTML={{ __html: svgHtml }}
                />
              )}

              <div
                className={mfIdentification.barcodeSlot}
                data-enabled={barcodeEnabled ? "true" : "false"}
                aria-hidden={!barcodeEnabled}
              >
                {barcodeEnabled ? (
                  <p className="text-center font-mono text-xs text-text-muted">{barcode?.value}</p>
                ) : null}
              </div>

              <p className={mfIdentification.primaryCode}>{context.primaryCode}</p>
              <p className={mfIdentification.entityName}>{context.name}</p>
              {(context.metadata ?? []).map((row) => (
                <p key={`${row.label}:${row.value}`} className={mfIdentification.metaRow}>
                  <span className="font-medium text-text-secondary">{row.label}: </span>
                  {row.value}
                </p>
              ))}
            </div>

            {exportError ? (
              <p className="mt-3 text-center text-xs text-status-danger" role="alert">
                {exportError}
              </p>
            ) : null}
          </div>

          <DialogFooter className={mfIdentification.footer}>
            <ExportActions
              onClose={() => onOpenChange(false)}
              onPrint={handlePrint}
              onDownloadPng={handleDownload}
              busy={exportBusy}
              disabled={!canExport}
              labels={{
                close: labels.close,
                print: labels.printLabel,
                downloadPng: labels.downloadPng,
              }}
            />
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {toasts.map((tm) => {
        const path = tm.variant === "success" && tm.description ? tm.description : null;
        return (
          <Toast
            key={tm.id}
            {...(tm.variant ? { variant: tm.variant } : {})}
            open
            onOpenChange={(isOpen) => {
              if (!isOpen) dismiss(tm.id);
            }}
          >
            <div className="grid gap-1">
              <ToastTitle>{tm.title}</ToastTitle>
              {tm.description ? (
                <ToastDescription className="break-all font-mono text-[11px]">
                  {tm.description}
                </ToastDescription>
              ) : null}
            </div>
            {path ? (
              <ToastAction asChild altText={t("qr.export.openFolder")}>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void handleOpenFolder(path)}
                >
                  {t("qr.export.openFolder")}
                </Button>
              </ToastAction>
            ) : null}
            <ToastClose />
          </Toast>
        );
      })}
      <ToastViewport />
    </ToastProvider>
  );
}
