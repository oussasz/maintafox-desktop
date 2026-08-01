/**
 * WoDetailDialog.tsx
 *
 * Full detail dialog for a work order (Option B lifecycle).
 * Single footer action line owns lifecycle CTAs; panels own form fields only.
 *
 * Tab visibility:
 *   Plan        — always (draft = submit hint only)
 *   Execution   — ready, in_progress, on_hold, completed, closed
 *   Close-out   — completed, closed
 *   Audit / Attachments — always (attachments blocked when closed/cancelled)
 */

import {
  CheckCircle2,
  ClipboardCheck,
  FileCheck2,
  History,
  Paperclip,
  Pause,
  Pencil,
  Play,
  Printer,
  RotateCcw,
  Settings,
  X,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { WoAttachmentPanel } from "@/components/wo/WoAttachmentPanel";
import { WoAuditTimeline } from "@/components/wo/WoAuditTimeline";
import { WoCancelDialog } from "@/components/wo/WoCancelDialog";
import { WoCloseOutPanel } from "@/components/wo/WoCloseOutPanel";
import { WoCompletionDialog } from "@/components/wo/WoCompletionDialog";
import { completionGatesProgress } from "@/components/wo/WoCompletionGatesChecklist";
import { WoCostSummaryCard } from "@/components/wo/WoCostSummaryCard";
import {
  WoExecutionControls,
  type WoExecutionControlsHandle,
} from "@/components/wo/WoExecutionControls";
import { WoPlanningPanel } from "@/components/wo/WoPlanningPanel";
import { printWoFiche } from "@/components/wo/WoPrintFiche";
import { usePermissions } from "@/hooks/use-permissions";
import { needsPlanningApproval, useWoLifecycleActions } from "@/hooks/use-wo-lifecycle-actions";
import { evaluateWoCompletionGates } from "@/services/wo-service";
import { pushAppToast } from "@/store/app-toast-store";
import { useWoStore } from "@/stores/wo-store";
import { formatDate } from "@/utils/format-date";
import { statusToI18nKey, STATUS_STYLE, URGENCY_STYLE } from "@/utils/wo-status";
import type { WoCompletionGate, WoStatus, WorkOrder } from "@shared/ipc-types";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

const EXECUTION_VISIBLE: Set<string> = new Set([
  "ready",
  "in_progress",
  "on_hold",
  "completed",
  "closed",
]);

const CLOSEOUT_VISIBLE: Set<string> = new Set(["in_progress", "on_hold", "completed", "closed"]);
const CANCELLABLE_DENY = new Set(["closed", "cancelled"]);
const ATTACHMENT_UPLOAD_DENY = new Set(["closed", "cancelled"]);

interface WoDetailDialogProps {
  wo: WorkOrder | null;
  open: boolean;
  loading?: boolean;
  onClose: () => void;
}

export function WoDetailDialog({ wo, open, loading = false, onClose }: WoDetailDialogProps) {
  const { t, i18n } = useTranslation("ot");
  const { can } = usePermissions();

  const saving = useWoStore((s) => s.saving);
  const openCreateForm = useWoStore((s) => s.openCreateForm);
  const openCompletionDialog = useWoStore((s) => s.openCompletionDialog);

  const [showCancelDialog, setShowCancelDialog] = useState(false);
  const [showReadinessDialog, setShowReadinessDialog] = useState(false);
  const [readinessDialogBlocking, setReadinessDialogBlocking] = useState<string[]>([]);
  const [closeoutGates, setCloseoutGates] = useState<WoCompletionGate[]>([]);
  const executionRef = useRef<WoExecutionControlsHandle>(null);

  const lifecycle = useWoLifecycleActions(wo);

  useEffect(() => {
    if (wo?.status_code === "planning") {
      void lifecycle.refreshReadiness();
    }
    // Only re-run when WO id/status changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wo?.id, wo?.status_code, wo?.row_version]);

  const showExecution = wo ? EXECUTION_VISIBLE.has(wo.status_code ?? "") : false;
  const showCloseout = wo ? CLOSEOUT_VISIBLE.has(wo.status_code ?? "") : false;

  useEffect(() => {
    if (!wo || !showCloseout) {
      setCloseoutGates([]);
      return;
    }
    let cancelled = false;
    void evaluateWoCompletionGates(wo.id)
      .then((rows) => {
        if (!cancelled) setCloseoutGates(rows);
      })
      .catch(() => {
        if (!cancelled) setCloseoutGates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [wo, wo?.id, wo?.row_version, wo?.parts_actuals_confirmed, showCloseout]);

  const closeoutProgress = useMemo(() => completionGatesProgress(closeoutGates), [closeoutGates]);

  const handlePrint = useCallback(() => {
    if (wo) void printWoFiche(wo, t, i18n.resolvedLanguage || i18n.language || "fr");
  }, [wo, t, i18n.resolvedLanguage, i18n.language]);

  const canEditWo = can(P.OT_EDIT);
  const canUploadAttachments =
    canEditWo && wo != null && !ATTACHMENT_UPLOAD_DENY.has(wo.status_code ?? "");

  const computedDefaultTab = useMemo(() => {
    if (!wo) return "plan";
    const sc = wo.status_code ?? "";
    if (sc === "cancelled") return "audit";
    if (sc === "ready" || sc === "in_progress" || sc === "on_hold") return "execution";
    if (CLOSEOUT_VISIBLE.has(sc)) return "closeout";
    return "plan";
  }, [wo]);

  const [activeTab, setActiveTab] = useState(computedDefaultTab);

  useEffect(() => {
    setActiveTab(computedDefaultTab);
  }, [computedDefaultTab]);

  const handleMarkReady = useCallback(async () => {
    const result = await lifecycle.markReady();
    if (result.ok) return;
    setReadinessDialogBlocking(result.blocking);
    setShowReadinessDialog(true);
    setActiveTab("plan");
    pushAppToast({
      title: t("planning.readinessBlocking"),
      ...(result.blocking.length > 0
        ? { description: result.blocking.slice(0, 3).join(" · ") }
        : {}),
      variant: "destructive",
    });
  }, [lifecycle, t]);

  if (!open) return null;

  if (loading && !wo) {
    return (
      <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
        <DialogContent className="w-full max-w-5xl max-h-[90vh] flex flex-col p-0 gap-0">
          <DialogHeader className="px-6 pt-5 pb-3">
            <DialogTitle>{t("detail.loading")}</DialogTitle>
            <DialogDescription>{t("detail.loadingHint")}</DialogDescription>
          </DialogHeader>
          <div className="flex flex-1 items-center justify-center px-6 py-16 text-sm text-muted-foreground">
            {t("detail.loading")}
          </div>
        </DialogContent>
      </Dialog>
    );
  }

  if (!wo) return null;

  const statusKey = statusToI18nKey(wo.status_code ?? "draft");
  const isCancelled = (wo.status_code ?? "") === "cancelled";
  const footerBusy = saving || lifecycle.busy;

  return (
    <>
      <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
        <DialogContent
          className="w-full max-w-5xl max-h-[90vh] flex flex-col p-0 gap-0"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          <DialogHeader className="px-6 pt-5 pb-3">
            <div className="flex items-start justify-between gap-4">
              <div className="space-y-1 min-w-0">
                <DialogTitle className="text-lg font-bold flex items-center gap-2">
                  <span className="text-muted-foreground font-mono text-base">{wo.code}</span>
                  <span className="truncate">{wo.title}</span>
                </DialogTitle>
                <DialogDescription className="text-sm text-muted-foreground line-clamp-2">
                  {wo.description}
                </DialogDescription>
              </div>
              <div className="flex items-center gap-1.5 shrink-0 pt-0.5">
                <Badge
                  variant="outline"
                  className={`text-xs border-0 ${STATUS_STYLE[wo.status_code ?? ""] ?? "bg-gray-100"}`}
                >
                  {t(`status.${statusKey}` as const)}
                </Badge>
                {wo.urgency_id != null && (
                  <Badge
                    variant="outline"
                    className={`text-xs border-0 ${URGENCY_STYLE[String(wo.urgency_id)] ?? ""}`}
                  >
                    {wo.urgency_label ?? t("form.urgency.label")}
                  </Badge>
                )}
              </div>
            </div>
          </DialogHeader>

          <Separator />

          <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
            {isCancelled && (
              <div className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm">
                <p className="font-medium text-destructive">{t("detail.cancelledBanner")}</p>
                {wo.cancel_reason ? (
                  <p className="mt-1 text-muted-foreground">{wo.cancel_reason}</p>
                ) : null}
                {wo.cancelled_at ? (
                  <p className="mt-1 text-xs text-muted-foreground">
                    {formatDate(wo.cancelled_at, i18n.language)}
                  </p>
                ) : null}
              </div>
            )}

            {lifecycle.error && (
              <div className="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
                {lifecycle.error}
              </div>
            )}

            <Card>
              <CardContent className="p-3 grid grid-cols-2 sm:grid-cols-3 gap-x-4 gap-y-2 text-xs">
                <InfoRow label={t("detail.fields.type")} value={wo.type_label ?? "—"} />
                <InfoRow label={t("detail.fields.equipment")} value={wo.asset_label ?? "—"} />
                <InfoRow
                  label={t("detail.fields.assignedTo")}
                  value={wo.responsible_display_name ?? wo.responsible_username ?? "—"}
                />
                <InfoRow
                  label={t("detail.fields.planner")}
                  value={wo.planner_display_name ?? wo.planner_username ?? "—"}
                />
                <InfoRow
                  label={t("detail.fields.plannedStart")}
                  value={wo.planned_start ? formatDate(wo.planned_start, i18n.language) : "—"}
                />
                <InfoRow
                  label={t("detail.fields.plannedEnd")}
                  value={wo.planned_end ? formatDate(wo.planned_end, i18n.language) : "—"}
                />
                <InfoRow
                  label={t("detail.fields.shift")}
                  value={wo.shift ? t(`shift.${wo.shift}`) : "—"}
                />
                <InfoRow
                  label={t("detail.fields.estimatedHours")}
                  value={
                    wo.expected_duration_hours != null ? `${wo.expected_duration_hours}h` : "—"
                  }
                />
                {(wo.source_di_code ?? "").trim() !== "" && (
                  <InfoRow
                    label={t("detail.fields.sourceDi")}
                    value={
                      <LinkedEntityBadge
                        entity="di"
                        code={wo.source_di_code}
                        entityId={wo.source_di_id}
                        title={wo.source_di_title}
                      />
                    }
                  />
                )}
                {wo.source_ram_ishikawa_diagram_id != null && wo.source_rca_cause_text && (
                  <InfoRow
                    label={t("detail.fields.sourceRca")}
                    value={
                      <span className="text-text-secondary">
                        {t("detail.fields.sourceRcaCause")}: {wo.source_rca_cause_text}
                      </span>
                    }
                  />
                )}
              </CardContent>
            </Card>

            <WoCostSummaryCard woId={wo.id} status={wo.status_code ?? "draft"} />

            <Tabs value={activeTab} onValueChange={setActiveTab}>
              <TabsList className="w-full justify-start">
                <TabsTrigger value="plan" className="gap-1.5 text-xs">
                  <Settings className="h-3.5 w-3.5" />
                  {t("detail.sections.planning")}
                </TabsTrigger>
                {showExecution && (
                  <TabsTrigger value="execution" className="gap-1.5 text-xs">
                    <Play className="h-3.5 w-3.5" />
                    {t("execution.title")}
                  </TabsTrigger>
                )}
                {showCloseout && (
                  <TabsTrigger value="closeout" className="gap-1.5 text-xs">
                    <ClipboardCheck className="h-3.5 w-3.5" />
                    {t("detail.sections.closeout")}
                    {closeoutProgress.total > 0 && (
                      <Badge variant="secondary" className="ml-1 h-5 px-1.5 text-[10px]">
                        {closeoutProgress.done}/{closeoutProgress.total}
                      </Badge>
                    )}
                  </TabsTrigger>
                )}
                <TabsTrigger value="audit" className="gap-1.5 text-xs">
                  <History className="h-3.5 w-3.5" />
                  {t("detail.sections.history")}
                </TabsTrigger>
                <TabsTrigger value="attachments" className="gap-1.5 text-xs">
                  <Paperclip className="h-3.5 w-3.5" />
                  {t("execution.attachments")}
                </TabsTrigger>
              </TabsList>

              <TabsContent value="plan" className="pt-3">
                <WoPlanningPanel wo={wo} canEdit={canEditWo} />
              </TabsContent>

              {showExecution && (
                <TabsContent value="execution" className="pt-3">
                  <WoExecutionControls ref={executionRef} wo={wo} canEdit={canEditWo} />
                </TabsContent>
              )}

              {showCloseout && (
                <TabsContent value="closeout" className="pt-3">
                  <WoCloseOutPanel wo={wo} canEdit={canEditWo} onClosed={onClose} />
                </TabsContent>
              )}

              <TabsContent value="audit" className="pt-3">
                <WoAuditTimeline woId={wo.id} />
              </TabsContent>

              <TabsContent value="attachments" className="pt-3">
                <WoAttachmentPanel
                  woId={wo.id}
                  canUpload={canUploadAttachments}
                  canDelete={canUploadAttachments}
                />
              </TabsContent>
            </Tabs>
          </div>

          <Separator />
          {lifecycle.readinessBlocking.length > 0 && (wo.status_code ?? "") === "planning" && (
            <div className="mx-6 mt-3 rounded-md border border-amber-300 bg-amber-50 px-3 py-2 text-sm text-amber-800">
              <p className="mb-1 font-medium">{t("planning.readinessBlocking")}</p>
              <ul className="list-disc list-inside space-y-0.5">
                {lifecycle.readinessBlocking.map((msg, i) => (
                  <li key={i}>{msg}</li>
                ))}
              </ul>
            </div>
          )}
          {/* Single footer action line */}
          <div className="flex flex-wrap items-center justify-between gap-2 px-6 py-3">
            <Button variant="outline" size="sm" onClick={handlePrint} className="gap-1.5">
              <Printer className="h-3.5 w-3.5" />
              {t("action.print")}
            </Button>
            <div className="flex flex-wrap items-center justify-end gap-2">
              <FooterActions
                wo={wo}
                busy={footerBusy}
                can={can}
                t={t}
                needsApproval={
                  lifecycle.readinessReport
                    ? needsPlanningApproval(lifecycle.readinessReport.checks)
                    : false
                }
                onEdit={() => {
                  openCreateForm(wo);
                  onClose();
                }}
                onSubmit={() => void lifecycle.submit()}
                onApprove={() => void lifecycle.approve()}
                onMarkReady={() => void handleMarkReady()}
                onReturnToPlanning={() => void lifecycle.returnToPlan()}
                onStart={() => {
                  setActiveTab("execution");
                  void lifecycle.start();
                }}
                onHold={() => {
                  setActiveTab("execution");
                  executionRef.current?.openHoldForm();
                }}
                onResume={() => void lifecycle.resume()}
                onComplete={openCompletionDialog}
                onCancel={() => setShowCancelDialog(true)}
              />
              <Button variant="outline" size="sm" onClick={onClose} className="gap-1.5">
                <X className="h-3.5 w-3.5" />
                {t("detail.close")}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>

      <Dialog open={showReadinessDialog} onOpenChange={setShowReadinessDialog}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("planning.readinessBlocking")}</DialogTitle>
            <DialogDescription>{t("planning.readinessBlockingHint")}</DialogDescription>
          </DialogHeader>
          {readinessDialogBlocking.length > 0 ? (
            <ul className="list-disc list-inside space-y-1 text-sm text-amber-900">
              {readinessDialogBlocking.map((msg, i) => (
                <li key={i}>{msg}</li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted-foreground">{t("planning.readinessBlockingEmpty")}</p>
          )}
          <div className="flex justify-end">
            <Button size="sm" onClick={() => setShowReadinessDialog(false)}>
              {t("detail.close")}
            </Button>
          </div>
        </DialogContent>
      </Dialog>

      <WoCompletionDialog wo={wo} />
      <WoCancelDialog wo={wo} open={showCancelDialog} onOpenChange={setShowCancelDialog} />
    </>
  );
}

interface FooterActionsProps {
  wo: WorkOrder;
  busy: boolean;
  can: (p: PermissionName) => boolean;
  t: (key: string) => string;
  needsApproval: boolean;
  onEdit: () => void;
  onSubmit: () => void;
  onApprove: () => void;
  onMarkReady: () => void;
  onReturnToPlanning: () => void;
  onStart: () => void;
  onHold: () => void;
  onResume: () => void;
  onComplete: () => void;
  onCancel: () => void;
}

function FooterActions({
  wo,
  busy,
  can,
  t,
  needsApproval,
  onEdit,
  onSubmit,
  onApprove,
  onMarkReady,
  onReturnToPlanning,
  onStart,
  onHold,
  onResume,
  onComplete,
  onCancel,
}: FooterActionsProps) {
  const s = (wo.status_code ?? "") as WoStatus;

  return (
    <>
      {s === "draft" && can(P.OT_EDIT) && (
        <>
          <Button size="sm" variant="outline" onClick={onEdit} disabled={busy} className="gap-1.5">
            <Pencil className="h-3.5 w-3.5" />
            {t("action.edit")}
          </Button>
          <Button size="sm" onClick={onSubmit} disabled={busy} className="gap-1.5">
            <Settings className="h-3.5 w-3.5" />
            {t("planning.submit")}
          </Button>
        </>
      )}

      {s === "planning" && can(P.OT_EDIT) && (
        <>
          {needsApproval && (
            <Button
              size="sm"
              variant="outline"
              onClick={onApprove}
              disabled={busy}
              className="gap-1.5"
            >
              <FileCheck2 className="h-3.5 w-3.5" />
              {t("planning.approvePlanning")}
            </Button>
          )}
          <Button
            size="sm"
            onClick={onMarkReady}
            disabled={busy}
            className="gap-1.5 bg-indigo-600 hover:bg-indigo-700 text-white"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            {t("planning.markReady")}
          </Button>
        </>
      )}

      {s === "ready" && can(P.OT_EDIT) && (
        <>
          <Button
            size="sm"
            variant="outline"
            onClick={onReturnToPlanning}
            disabled={busy}
            className="gap-1.5"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            {t("planning.returnToPlanning")}
          </Button>
          <Button
            size="sm"
            onClick={onStart}
            disabled={busy}
            className="gap-1.5 bg-green-600 hover:bg-green-700 text-white"
          >
            <Play className="h-3.5 w-3.5" />
            {t("action.start")}
          </Button>
        </>
      )}

      {s === "in_progress" && can(P.OT_EDIT) && (
        <>
          <Button size="sm" variant="outline" onClick={onHold} disabled={busy} className="gap-1.5">
            <Pause className="h-3.5 w-3.5" />
            {t("action.hold")}
          </Button>
          <Button
            size="sm"
            onClick={onComplete}
            disabled={busy}
            className="gap-1.5 bg-amber-600 hover:bg-amber-700 text-white"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            {t("action.complete")}
          </Button>
        </>
      )}

      {s === "on_hold" && can(P.OT_EDIT) && (
        <Button
          size="sm"
          onClick={onResume}
          disabled={busy}
          className="gap-1.5 bg-blue-600 hover:bg-blue-700 text-white"
        >
          <Play className="h-3.5 w-3.5" />
          {t("footer.resume")}
        </Button>
      )}

      {!CANCELLABLE_DENY.has(s) && can(P.OT_CLOSE) && (
        <Button
          size="sm"
          variant="outline"
          onClick={onCancel}
          disabled={busy}
          className="gap-1.5 text-destructive"
        >
          <XCircle className="h-3.5 w-3.5" />
          {t("action.cancelWo")}
        </Button>
      )}
    </>
  );
}

function InfoRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-muted-foreground">{label}:</span>
      <span className="font-medium">{value}</span>
    </div>
  );
}
