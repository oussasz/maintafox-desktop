/**
 * DiApprovalDialog.tsx
 *
 * Approval confirmation modal with DI summary, conversion preview,
 * approval note, and print button. Follows UX-DW-001 pattern.
 *
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S4.
 */

import type { TFunction } from "i18next";
import { CheckCircle2, Loader2, Printer, TriangleAlert } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { UrgencyBadge } from "@/components/common/urgency/UrgencyBadge";
import { urgencyLabelFor } from "@/components/common/urgency/use-urgency-scale";
import {
  fetchDiSymptomLabel,
  loadDiOriginLabelMap,
  resolveDiOriginLabel,
  useDiReferenceLabels,
} from "@/components/di/di-reference-labels";
import { DI_STATUS_STYLE, diStatusToI18nKey } from "@/components/di/status-meta";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
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
import { printHtml } from "@/export/print-manager";
import { useAccessAt } from "@/hooks/use-access-at";
import { useSession } from "@/hooks/use-session";
import { i18n } from "@/i18n";
import { describeAppError } from "@/lib/app-error";
import { formatAssetLabel, formatOrgNodeLabel, formatPersonLabel } from "@/lib/display";
import { convertDiToWo } from "@/services/di-conversion-service";
import { pushAppToast } from "@/store/app-toast-store";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { intlLocaleForLanguage } from "@/utils/format-date";
import type { InterventionRequest, WoConversionResult } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import { permitFlagMessages } from "./permit-flag-text";

// ── Helpers ─────────────────────────────────────────────────────────────────

function esc(v: string): string {
  return v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ── Print ───────────────────────────────────────────────────────────────────

function printApprovalSheet(
  di: InterventionRequest,
  approverName: string,
  note: string,
  t: TFunction<"di">,
  locale: string,
  originLabel: string,
  symptomLabel: string,
  urgencyLabel: string,
) {
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
    <tr><th>${t("approvalPrint.thStatus")}</th><td>${esc(t(`status.${diStatusToI18nKey(di.status)}`))}</td></tr>
    <tr><th>${t("approvalPrint.thPriority")}</th><td>${esc(urgencyLabel)}</td></tr>
    <tr><th>${t("approvalPrint.thOrigin")}</th><td>${esc(originLabel)}</td></tr>
    ${
      symptomLabel
        ? `<tr><th>${t("detail.fields.symptom")}</th><td>${esc(symptomLabel)}</td></tr>`
        : ""
    }
    <tr><th>${t("approvalPrint.thImpact")}</th><td>${esc(t(`form.impact.${di.impact_level}` as "form.impact.unknown", { defaultValue: di.impact_level }))}</td></tr>
    <tr><th>${t("approvalPrint.thAsset")}</th><td>${esc(formatAssetLabel(di.asset_code, di.asset_label))}</td></tr>
    <tr><th>${t("approvalPrint.thOrg")}</th><td>${esc(formatOrgNodeLabel(di.org_node_code, di.org_node_label))}</td></tr>
    <tr><th>${t("approvalPrint.thSubmitted")}</th><td>${esc(fmt(di.submitted_at))}</td></tr>
    <tr><th>${t("approvalPrint.thDescription")}</th><td>${esc(di.description)}</td></tr>
    ${
      di.reviewer_note
        ? `<tr><th>${t("approvalPrint.thReviewerNote")}</th><td>${esc(di.reviewer_note)}</td></tr>`
        : ""
    }
    ${note ? `<tr><th>${t("approvalPrint.thApprovalNote")}</th><td>${esc(note)}</td></tr>` : ""}
  </table>

  <div class="conversion">${t("approvalPrint.conversion", { from: esc(di.code) })}</div>

  <div class="signatures">
    <div class="sig-box">
      <p><strong>${t("approvalPrint.sigDeclarant")}</strong></p>
      <p>${esc(formatPersonLabel(di.submitter_display_name))}</p>
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

  printHtml(html, { title: di.code }).catch((err: unknown) => {
    console.error("[DiApprovalDialog] print failed", err);
  });
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiApprovalDialog() {
  const { t, i18n } = useTranslation("di");
  const navigate = useNavigate();
  const dateLocale = intlLocaleForLanguage(i18n.language);
  const { originLabel, symptomLabel } = useDiReferenceLabels({ includeSymptoms: true });
  const di = useDiReviewStore((s) => s.approvalDi);
  const closeApproval = useDiReviewStore((s) => s.closeApproval);
  const openRejection = useDiReviewStore((s) => s.openRejection);
  const approve = useDiReviewStore((s) => s.approve);
  const saving = useDiReviewStore((s) => s.saving);
  const storeError = useDiReviewStore((s) => s.error);
  // Rights at this request's site; converting also creates the work order there.
  const { can } = useAccessAt(di?.org_node_id);
  const { info } = useSession();

  const [note, setNote] = useState("");
  // Approve and convert are confirmed in a dialog (no password re-entry; RBAC + audit on the backend).
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [confirmMode, setConfirmMode] = useState<"approve" | "convert">("approve");
  const [conversionResult, setConversionResult] = useState<{
    woId: number | null;
    woCode: string | null;
    conversionError: string | null;
  } | null>(null);
  const [retryRowVersion, setRetryRowVersion] = useState<number | null>(null);

  const loadDis = useDiStore((s) => s.loadDis);

  const open = di !== null;
  const approverName = info?.display_name ?? info?.username ?? "";

  const handoffToWorkOrder = useCallback(
    (woId: number) => {
      setConversionResult(null);
      setNote("");
      setConfirmOpen(false);
      setRetryRowVersion(null);
      closeApproval();
      navigate(`/work-orders?openWo=${woId}`);
    },
    [closeApproval, navigate],
  );

  /** What the conversion decided about the new work order's permit (rules, safety flag). */
  const announcePermitFlag = useCallback(
    (result: WoConversionResult) => {
      const tText = t as unknown as (key: string, options?: Record<string, unknown>) => string;
      for (const message of permitFlagMessages(result.permit_flag, result.wo_code, tText)) {
        pushAppToast({
          title: message.title,
          variant: message.variant,
          ...(message.description ? { description: message.description } : {}),
          ...(message.persistent ? { persistent: true } : {}),
        });
      }
    },
    [t],
  );

  const handleApproveClick = useCallback(() => {
    setConfirmMode("approve");
    setConfirmOpen(true);
  }, []);

  const handleConvertClick = useCallback(() => {
    setConfirmMode("convert");
    setConfirmOpen(true);
  }, []);

  const handleCloseRequest = useCallback(() => {
    if (!di) return;
    closeApproval();
    openRejection(di);
  }, [closeApproval, di, openRejection]);

  const handleConfirmed = useCallback(async () => {
    if (!di) return;
    if (confirmMode === "approve") {
      try {
        const result = await approve({
          di_id: di.id,
          actor_id: info?.user_id ?? 0,
          expected_row_version: di.row_version,
          notes: note || null,
        });
        void loadDis();
        setRetryRowVersion(result.approvedRowVersion);
        setConversionResult(null);
      } catch {
        // error is set in store; dialog stays open so user can retry
      }
      return;
    }

    try {
      const result = await convertDiToWo({
        diId: di.id,
        expectedRowVersion: retryRowVersion ?? di.row_version,
        ...(note ? { conversionNotes: note } : {}),
      });
      void loadDis();
      announcePermitFlag(result);
      if (result.wo_id > 0) {
        handoffToWorkOrder(result.wo_id);
        return;
      }
      setConversionResult({
        woId: result.wo_id,
        woCode: result.wo_code,
        conversionError: t(
          "review.conversionMissingWoId",
          "Conversion succeeded but work order id is missing — open Work Orders manually.",
        ),
      });
      setRetryRowVersion(result.di.row_version);
    } catch (err) {
      setConversionResult({
        woId: null,
        woCode: null,
        conversionError: describeAppError(err).title,
      });
    }
  }, [
    di,
    approve,
    info,
    note,
    handoffToWorkOrder,
    announcePermitFlag,
    loadDis,
    confirmMode,
    retryRowVersion,
    t,
  ]);

  const handleRetryConversion = useCallback(async () => {
    if (!di || retryRowVersion == null) return;
    try {
      const result = await convertDiToWo({
        diId: di.id,
        expectedRowVersion: retryRowVersion,
        ...(note ? { conversionNotes: note } : {}),
      });
      void loadDis();
      announcePermitFlag(result);
      if (result.wo_id > 0) {
        handoffToWorkOrder(result.wo_id);
        return;
      }
      setConversionResult({
        woId: result.wo_id,
        woCode: result.wo_code,
        conversionError: null,
      });
      setRetryRowVersion(result.di.row_version);
    } catch (err) {
      setConversionResult({
        woId: null,
        woCode: null,
        conversionError: describeAppError(err).title,
      });
    }
  }, [di, handoffToWorkOrder, announcePermitFlag, loadDis, note, retryRowVersion]);

  const handlePrint = useCallback(() => {
    if (!di) return;
    const tPrint = i18n.getFixedT(i18n.language, "di");
    void (async () => {
      const [originMap, symptomText, urgencyText] = await Promise.all([
        loadDiOriginLabelMap(),
        fetchDiSymptomLabel(di.symptom_code_id),
        urgencyLabelFor(di.validated_urgency ?? di.reported_urgency, i18n.language),
      ]);
      printApprovalSheet(
        di,
        approverName,
        note,
        tPrint,
        intlLocaleForLanguage(i18n.language),
        resolveDiOriginLabel(originMap, di.origin_type),
        symptomText,
        urgencyText,
      );
    })();
  }, [di, approverName, note, i18n]);

  const handleClose = useCallback(() => {
    setNote("");
    setConfirmOpen(false);
    setConversionResult(null);
    setRetryRowVersion(null);
    closeApproval();
  }, [closeApproval]);

  if (!di) return null;

  return (
    <>
      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={
          confirmMode === "approve"
            ? t("review.confirmApproveTitle")
            : t("review.confirmConvertTitle")
        }
        description={
          confirmMode === "approve"
            ? t("review.confirmApproveDescription", { code: di.code })
            : t("review.confirmConvertDescription", { code: di.code })
        }
        confirmLabel={
          confirmMode === "approve"
            ? t("review.confirmApproveAction")
            : t("review.confirmConvertAction")
        }
        onConfirm={handleConfirmed}
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
            {/* Conversion notice — WO code is assigned only on convert */}
            <div className="rounded-lg bg-tone-success-bg border border-tone-success-border py-3 px-4 space-y-1">
              <p className="text-center font-mono font-bold text-tone-success-fg">{di.code}</p>
              <p className="text-center text-sm text-tone-success-fg">
                {t("review.woCodeAssignedOnConvert")}
              </p>
            </div>

            {/* DI info card */}
            <Card>
              <CardContent className="p-4 space-y-3">
                <div className="flex items-center gap-2">
                  <span className="font-mono text-sm text-muted-foreground">{di.code}</span>
                  <span className="font-semibold text-sm">{di.title}</span>
                  <LinkedEntityBadge
                    entity="work_order"
                    code={conversionResult?.woCode ?? di.converted_to_wo_code}
                    entityId={conversionResult?.woId ?? di.converted_to_wo_id}
                    title={di.converted_to_wo_title}
                    className="text-[10px] px-1.5 py-0"
                  />
                  <Badge
                    variant="outline"
                    className={`text-[10px] border-0 ml-auto ${DI_STATUS_STYLE[di.status] ?? "bg-tone-neutral-bg"}`}
                  >
                    {t(`status.${diStatusToI18nKey(di.status)}` as const)}
                  </Badge>
                  <UrgencyBadge code={di.validated_urgency ?? di.reported_urgency} />
                </div>

                {/* Equipment */}
                <div className="grid grid-cols-3 gap-2 text-xs">
                  <div>
                    <span className="text-muted-foreground">{t("detail.fields.asset")}:</span>{" "}
                    <span className="font-medium">
                      {formatAssetLabel(di.asset_code, di.asset_label)}
                    </span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t("detail.fields.orgNode")}:</span>{" "}
                    <span className="font-medium">
                      {formatOrgNodeLabel(di.org_node_code, di.org_node_label)}
                    </span>
                  </div>
                  <div>
                    <span className="text-muted-foreground">{t("detail.fields.origin")}:</span>{" "}
                    <span className="font-medium">{originLabel(di.origin_type)}</span>
                  </div>
                  {di.symptom_code_id != null && (
                    <div>
                      <span className="text-muted-foreground">{t("detail.fields.symptom")}:</span>{" "}
                      <span className="font-medium">{symptomLabel(di.symptom_code_id)}</span>
                    </div>
                  )}
                </div>

                {/* Requester */}
                <div className="text-xs">
                  <span className="text-muted-foreground">{t("detail.fields.reportedBy")}:</span>{" "}
                  <span className="font-medium">
                    {formatPersonLabel(di.submitter_display_name)}
                  </span>
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
                  {new Date().toLocaleString(dateLocale)}
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

            {conversionResult?.woCode && !conversionResult.conversionError && (
              <div
                role="status"
                className="rounded-md bg-tone-success-bg border border-tone-success-border px-4 py-3 text-sm text-tone-success-fg flex items-center gap-2"
              >
                <CheckCircle2 className="h-4 w-4 shrink-0" />
                <span>
                  {t("review.conversionSuccess")}{" "}
                  <strong className="font-mono">{conversionResult.woCode}</strong>
                </span>
              </div>
            )}
            {conversionResult?.conversionError && (
              <div
                role="alert"
                className="rounded-md bg-tone-warning-bg border border-tone-warning-border px-4 py-3 text-sm text-tone-warning-fg flex items-start gap-2"
              >
                <TriangleAlert className="h-4 w-4 shrink-0 mt-0.5" />
                <div>
                  <p className="font-medium">{t("review.conversionFailed")}</p>
                  <p className="text-xs mt-1">{conversionResult.conversionError}</p>
                  {retryRowVersion != null && (
                    <div className="mt-2">
                      <Button
                        size="sm"
                        variant="outline"
                        onClick={() => void handleRetryConversion()}
                        className="h-7"
                      >
                        {t("review.retryConversion")}
                      </Button>
                    </div>
                  )}
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
              {di.status === "awaiting_approval" && (
                <Button
                  size="sm"
                  onClick={handleApproveClick}
                  disabled={saving}
                  className="gap-1.5"
                >
                  {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                  {t("action.approve")}
                </Button>
              )}
              {di.status === "approved" &&
                can(P.DI_CONVERT) &&
                can(P.OT_CREATE) &&
                di.converted_to_wo_id == null && (
                  <Button
                    size="sm"
                    onClick={handleConvertClick}
                    disabled={saving}
                    className="gap-1.5"
                  >
                    {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                    {t("review.convertToWo")}
                  </Button>
                )}
              {(di.status === "awaiting_approval" || di.status === "approved") && (
                <Button size="sm" variant="outline" onClick={handleCloseRequest}>
                  {t("action.closeRequest")}
                </Button>
              )}
            </div>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
