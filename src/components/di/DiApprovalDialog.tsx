/**
 * DiApprovalDialog.tsx
 *
 * Approval confirmation modal with DI summary, conversion preview,
 * approval note, and print button. Follows UX-DW-001 pattern.
 *
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S4.
 */

import type { TFunction } from "i18next";
import { ArrowRight, CheckCircle2, Loader2, Printer, TriangleAlert } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { StepUpDialog } from "@/components/auth/StepUpDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { i18n } from "@/i18n";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Helpers ─────────────────────────────────────────────────────────────────

const URGENCY_STYLE: Record<string, string> = {
  critical: "bg-red-100 text-red-700",
  high: "bg-orange-100 text-orange-800",
  medium: "bg-yellow-100 text-yellow-800",
  low: "bg-green-100 text-green-800",
};

const STATUS_STYLE: Record<string, string> = {
  pending_review: "bg-amber-100 text-amber-800",
  screened: "bg-sky-100 text-sky-800",
  awaiting_approval: "bg-yellow-100 text-yellow-800",
};

function esc(v: string): string {
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function generateWoCodePreview(diCode: string): string {
  return diCode.replace(/^DI-/, "OT-");
}

// ── Print ───────────────────────────────────────────────────────────────────

function printApprovalSheet(
  di: InterventionRequest,
  approverName: string,
  note: string,
  t: TFunction<"di">,
  locale: string,
) {
  const woCode = generateWoCodePreview(di.code);
  const now = new Date().toLocaleString(locale);
  const fmt = (iso: string) => {
    try {
      return new Date(iso).toLocaleString(locale, {
        day: "2-digit",
        month: "2-digit",
        year: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return iso;
    }
  };

  const html = `<!DOCTYPE html>
<html lang="${i18n.language.split("-")[0] ?? "fr"}">
<head>
  <meta charset="utf-8" />
  <title>${t("approvalPrint.title")}</title>
  <style>
    body { font-family: Arial, sans-serif; margin: 40px; font-size: 12px; }
    h1 { font-size: 18px; text-align: center; margin-bottom: 4px; }
    .subtitle { text-align: center; color: #666; margin-bottom: 24px; }
    table { width: 100%; border-collapse: collapse; margin-bottom: 16px; }
    td, th { border: 1px solid #ccc; padding: 6px 8px; text-align: left; }
    th { background: #f5f5f5; width: 30%; }
    .conversion { background: #e8f5e9; padding: 12px; text-align: center; font-weight: bold; margin: 16px 0; }
    .signatures { display: flex; gap: 40px; margin-top: 32px; }
    .sig-box { flex: 1; border-top: 1px solid #333; padding-top: 8px; text-align: center; }
    .footer { margin-top: 40px; font-size: 10px; color: #999; text-align: center; border-top: 1px solid #eee; padding-top: 8px; }
    @media print { body { margin: 20px; } }
  </style>
</head>
<body>
  <h1>${t("approvalPrint.title")}</h1>
  <p class="subtitle">${t("approvalPrint.subtitle", { code: esc(di.code), date: now })}</p>

  <table>
    <tr><th>${t("approvalPrint.thCode")}</th><td>${esc(di.code)}</td></tr>
    <tr><th>${t("approvalPrint.thTitle")}</th><td>${esc(di.title)}</td></tr>
    <tr><th>${t("approvalPrint.thStatus")}</th><td>${esc(di.status)}</td></tr>
    <tr><th>${t("approvalPrint.thPriority")}</th><td>${esc(di.reported_urgency)}</td></tr>
    <tr><th>${t("approvalPrint.thOrigin")}</th><td>${esc(di.origin_type)}</td></tr>
    <tr><th>${t("approvalPrint.thImpact")}</th><td>${esc(di.impact_level)}</td></tr>
    <tr><th>${t("approvalPrint.thAsset")}</th><td>#${di.asset_id}</td></tr>
    <tr><th>${t("approvalPrint.thOrg")}</th><td>#${di.org_node_id}</td></tr>
    <tr><th>${t("approvalPrint.thSubmitted")}</th><td>${esc(fmt(di.submitted_at))}</td></tr>
    <tr><th>${t("approvalPrint.thDescription")}</th><td>${esc(di.description)}</td></tr>
    ${
      di.reviewer_note
        ? `<tr><th>${t("approvalPrint.thReviewerNote")}</th><td>${esc(di.reviewer_note)}</td></tr>`
        : ""
    }
    ${note ? `<tr><th>${t("approvalPrint.thApprovalNote")}</th><td>${esc(note)}</td></tr>` : ""}
  </table>

  <div class="conversion">${t("approvalPrint.conversion", { from: esc(di.code), to: esc(woCode) })}</div>

  <div class="signatures">
    <div class="sig-box">
      <p><strong>${t("approvalPrint.sigDeclarant")}</strong></p>
      <p>#${di.submitter_id}</p>
      <br/><br/>
      <p>${t("print.signatureLine")}</p>
    </div>
    <div class="sig-box">
      <p><strong>${t("approvalPrint.sigApprover")}</strong></p>
      <p>${esc(approverName)}</p>
      <br/><br/>
      <p>${t("print.signatureLine")}</p>
    </div>
    <div class="sig-box">
      <p><strong>${t("approvalPrint.sigResp")}</strong></p>
      <br/><br/><br/>
      <p>${t("print.signatureLine")}</p>
    </div>
  </div>

  <div class="footer">
    ${t("approvalPrint.footer", { code: esc(di.code) })}
  </div>
</body>
</html>`;

  const w = window.open("", "_blank");
  if (w) {
    w.document.write(html);
    w.document.close();
    w.print();
  }
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiApprovalDialog() {
  const { t, i18n } = useTranslation("di");
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const di = useDiReviewStore((s) => s.approvalDi);
  const closeApproval = useDiReviewStore((s) => s.closeApproval);
  const approve = useDiReviewStore((s) => s.approve);
  const saving = useDiReviewStore((s) => s.saving);
  const storeError = useDiReviewStore((s) => s.error);
  const { info } = useSession();

  const [note, setNote] = useState("");
  const [showStepUp, setShowStepUp] = useState(false);
  const [conversionResult, setConversionResult] = useState<{
    converted: boolean;
    woCode: string | null;
    conversionError: string | null;
  } | null>(null);

  const loadDis = useDiStore((s) => s.loadDis);

  const open = di !== null;
  const woCodePreview = di ? generateWoCodePreview(di.code) : "";
  const approverName = info?.display_name ?? info?.username ?? "";

  // Step 1: user clicks "Approve" → show step-up dialog
  const handleApproveClick = useCallback(() => {
    setShowStepUp(true);
  }, []);

  // Step 2: step-up verified → actually run the approve action
  const handleStepUpVerified = useCallback(async () => {
    setShowStepUp(false);
    if (!di) return;
    try {
      const result = await approve({
        di_id: di.id,
        actor_id: info?.user_id ?? 0,
        expected_row_version: di.row_version,
        notes: note || null,
      });
      // Refresh DI list so Kanban updates
      void loadDis();
      if (result.converted) {
        // Show success with actual WO code, then auto-close after delay
        setConversionResult(result);
        setTimeout(() => {
          setConversionResult(null);
          setNote("");
          closeApproval();
        }, 3000);
      } else {
        // Approval succeeded but conversion failed — show warning, keep dialog open
        setConversionResult(result);
      }
    } catch {
      // error is set in store; dialog stays open so user can retry
    }
  }, [di, approve, info, note, closeApproval, loadDis]);

  const handleStepUpCancel = useCallback(() => {
    setShowStepUp(false);
  }, []);

  const handlePrint = useCallback(() => {
    if (di) {
      const tPrint = i18n.getFixedT(i18n.language, "di");
      printApprovalSheet(di, approverName, note, tPrint, intlLocaleForLanguage(i18n.language));
    }
  }, [di, approverName, note, i18n]);

  const handleClose = useCallback(() => {
    setNote("");
    setShowStepUp(false);
    setConversionResult(null);
    closeApproval();
  }, [closeApproval]);

  if (!di) return null;

  return (
    <>
      <StepUpDialog
        open={showStepUp}
        onVerified={handleStepUpVerified}
        onCancel={handleStepUpCancel}
      />
      <Dialog open={open} onOpenChange={(isOpen) => !isOpen && handleClose()}>
        <DialogContent
          className="max-w-3xl max-h-[85vh] flex flex-col p-0 gap-0"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader className="px-6 pt-5 pb-3">
            <DialogTitle className="text-lg font-bold">{t("review.approvalTitle")}</DialogTitle>
          </DialogHeader>

          <Separator />

          <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
            {/* Conversion banner */}
            <div className="flex items-center justify-center gap-3 rounded-lg bg-green-50 border border-green-200 py-3 px-4">
              <span className="font-mono font-bold text-green-800">{di.code}</span>
              <ArrowRight className="h-4 w-4 text-green-600" />
              <span className="font-mono font-bold text-green-800">{woCodePreview}</span>
            </div>

            {/* DI info card */}
            <Card>
              <CardContent className="p-4 space-y-3">
                <div className="flex items-center gap-2">
                  <span className="font-mono text-sm text-muted-foreground">{di.code}</span>
                  <span className="font-semibold text-sm">{di.title}</span>
                  <Badge
                    variant="outline"
                    className={`text-[10px] border-0 ml-auto ${STATUS_STYLE[di.status] ?? "bg-gray-100"}`}
                  >
                    {di.status}
                  </Badge>
                  <Badge
                    variant="outline"
                    className={`text-[10px] border-0 ${URGENCY_STYLE[di.reported_urgency] ?? ""}`}
                  >
                    {t(`priority.${di.reported_urgency}`)}
                  </Badge>
                </div>

                {/* Equipment */}
                <div className="grid grid-cols-3 gap-2 text-xs">
                  <div>
                    <span className="text-muted-foreground">{t("detail.fields.asset")}:</span>{" "}
                    <span className="font-medium">#{di.asset_id}</span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t("detail.fields.orgNode")}:</span>{" "}
                    <span className="font-medium">#{di.org_node_id}</span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t("detail.fields.origin")}:</span>{" "}
                    <span className="font-medium">{di.origin_type}</span>
                  </div>
                </div>

                {/* Requester */}
                <div className="text-xs">
                  <span className="text-muted-foreground">{t("detail.fields.reportedBy")}:</span>{" "}
                  <span className="font-medium">#{di.submitter_id}</span>
                  <span className="ml-4 text-muted-foreground">
                    {t("detail.fields.reportedAt")}:
                  </span>{" "}
                  <span className="font-medium">
                    {new Date(di.submitted_at).toLocaleString(dateLocale, {
                      day: "2-digit",
                      month: "2-digit",
                      year: "numeric",
                      hour: "2-digit",
                      minute: "2-digit",
                    })}
                  </span>
                </div>

                {/* Description */}
                <div className="text-xs">
                  <p className="text-muted-foreground mb-1">{t("detail.fields.description")}:</p>
                  <p className="whitespace-pre-wrap">{di.description}</p>
                </div>

                {di.reviewer_note && (
                  <div className="text-xs">
                    <p className="text-muted-foreground mb-1">{t("review.reviewerNote")}:</p>
                    <p className="whitespace-pre-wrap">{di.reviewer_note}</p>
                  </div>
                )}
              </CardContent>
            </Card>

            {/* Approval note */}
            <div className="space-y-2">
              <Label htmlFor="approval-note">{t("review.approvalNote")}</Label>
              <Textarea
                id="approval-note"
                value={note}
                onChange={(e) => setNote(e.target.value)}
                maxLength={2000}
                placeholder={t("review.approvalNotePlaceholder")}
                rows={3}
              />
              <p className="text-[10px] text-muted-foreground text-right">{note.length}/2000</p>
            </div>

            {/* Approver signature preview */}
            <Card>
              <CardContent className="p-3 flex items-center gap-4 text-xs">
                <span className="text-muted-foreground">{t("review.approver")}:</span>
                <span className="font-medium">{approverName}</span>
                <span className="ml-auto text-muted-foreground">
                  {new Date().toLocaleString("fr-FR")}
                </span>
              </CardContent>
            </Card>

            {/* Error feedback */}
            {storeError && (
              <div
                role="alert"
                className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive"
              >
                {storeError}
              </div>
            )}

            {/* Conversion result feedback */}
            {conversionResult?.converted && (
              <div
                role="status"
                className="rounded-md bg-green-100 border border-green-300 px-4 py-3 text-sm text-green-800 flex items-center gap-2"
              >
                <CheckCircle2 className="h-4 w-4 shrink-0" />
                <span>
                  {t("review.conversionSuccess", "DI approuvée et convertie en OT avec succès.")}{" "}
                  <strong className="font-mono">{conversionResult.woCode}</strong>
                </span>
              </div>
            )}
            {conversionResult && !conversionResult.converted && (
              <div
                role="alert"
                className="rounded-md bg-amber-100 border border-amber-300 px-4 py-3 text-sm text-amber-800 flex items-start gap-2"
              >
                <TriangleAlert className="h-4 w-4 shrink-0 mt-0.5" />
                <div>
                  <p className="font-medium">
                    {t(
                      "review.conversionFailed",
                      "DI approuvée mais la conversion en OT a échoué.",
                    )}
                  </p>
                  <p className="text-xs mt-1">{conversionResult.conversionError}</p>
                </div>
              </div>
            )}
          </div>

          <Separator />

          <DialogFooter className="px-6 py-3 flex items-center justify-between">
            <Button variant="outline" size="sm" onClick={handlePrint} className="gap-1.5">
              <Printer className="h-3.5 w-3.5" />
              {t("review.print")}
            </Button>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={handleClose}>
                {t("form.cancel")}
              </Button>
              <Button
                size="sm"
                onClick={handleApproveClick}
                disabled={saving}
                className="gap-1.5 bg-green-600 hover:bg-green-700 text-white"
              >
                {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                {t("review.approveAndConvert")}
              </Button>
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
