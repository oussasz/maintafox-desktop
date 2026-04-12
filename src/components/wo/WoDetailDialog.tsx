/**
 * WoDetailDialog.tsx
 *
 * Full detail dialog for a work order. UX-DW-001 pattern:
 *   • Centered modal overlay (max-w-4xl, max-h-[90vh])
 *   • Header: WO code + title + status/urgency badges + close
 *   • Body: scrollable info grid + 5-tab sub-panels
 *   • Footer: context-appropriate lifecycle action buttons
 *
 * Tab visibility:
 *   Plan        — always visible; editable only in draft/planned/ready_to_schedule
 *   Execution   — visible once assigned or later
 *   Close-out   — visible once mechanically_complete or later
 *   Audit       — always visible (read-only)
 *   Attachments — always visible
 *
 * Phase 2 – Sub-phase 05 – File 02 – Sprint S4.
 */

import {
  CheckCircle2,
  ClipboardCheck,
  FileText,
  History,
  Paperclip,
  Pause,
  Pencil,
  Play,
  Printer,
  Settings,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

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
import { WoCloseOutPanel } from "@/components/wo/WoCloseOutPanel";
import { WoCompletionDialog } from "@/components/wo/WoCompletionDialog";
import { WoCostSummaryCard } from "@/components/wo/WoCostSummaryCard";
import { WoExecutionControls } from "@/components/wo/WoExecutionControls";
import { WoPlanningPanel } from "@/components/wo/WoPlanningPanel";
import { printWoFiche } from "@/components/wo/WoPrintFiche";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import { useWoStore } from "@/stores/wo-store";
import { formatDate } from "@/utils/format-date";
import { statusToI18nKey, STATUS_STYLE, URGENCY_STYLE } from "@/utils/wo-status";
import type { WoStatus, WorkOrder } from "@shared/ipc-types";

// ── Status groupings for tab visibility ─────────────────────────────────────

const EXECUTION_VISIBLE: Set<string> = new Set([
  "assigned",
  "waiting_for_prerequisite",
  "in_progress",
  "paused",
  "on_hold",
  "mechanically_complete",
  "technically_verified",
  "closed",
  "cancelled",
]);

const CLOSEOUT_VISIBLE: Set<string> = new Set([
  "mechanically_complete",
  "technically_verified",
  "closed",
]);

// ── Props ───────────────────────────────────────────────────────────────────

interface WoDetailDialogProps {
  wo: WorkOrder | null;
  open: boolean;
  onClose: () => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function WoDetailDialog({ wo, open, onClose }: WoDetailDialogProps) {
  const { t, i18n } = useTranslation("ot");
  const { can } = usePermissions();
  const { info } = useSession();

  const saving = useWoStore((s) => s.saving);
  const openCreateForm = useWoStore((s) => s.openCreateForm);
  const openCompletionDialog = useWoStore((s) => s.openCompletionDialog);
  const closeWorkOrder = useWoStore((s) => s.closeWorkOrder);

  const handlePrint = useCallback(() => {
    if (wo) void printWoFiche(wo, t, i18n.resolvedLanguage || i18n.language || "fr");
  }, [wo, t, i18n.resolvedLanguage, i18n.language]);

  // Determine visible tabs
  const showExecution = wo ? EXECUTION_VISIBLE.has(wo.status_code ?? "") : false;
  const showCloseout = wo ? CLOSEOUT_VISIBLE.has(wo.status_code ?? "") : false;
  const canEditWo = can("ot.edit");

  // Determine default tab
  const computedDefaultTab = useMemo(() => {
    if (!wo) return "plan";
    const sc = wo.status_code ?? "";
    if (
      sc === "in_progress" ||
      sc === "on_hold" ||
      sc === "paused" ||
      sc === "waiting_for_prerequisite"
    )
      return "execution";
    if (CLOSEOUT_VISIBLE.has(sc)) return "closeout";
    return "plan";
  }, [wo]);

  const [activeTab, setActiveTab] = useState(computedDefaultTab);

  // Sync active tab when WO status changes (e.g. after start/pause from within panel)
  useEffect(() => {
    setActiveTab(computedDefaultTab);
  }, [computedDefaultTab]);

  if (!wo) return null;

  const statusKey = statusToI18nKey(wo.status_code ?? "draft");

  return (
    <>
      <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
        <DialogContent
          className="max-w-4xl max-h-[90vh] flex flex-col p-0 gap-0"
          onPointerDownOutside={(e) => e.preventDefault()}
        >
          {/* ── Header ──────────────────────────────────────────────── */}
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

          {/* ── Scrollable body ─────────────────────────────────────── */}
          <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
            {/* Info grid */}
            <Card>
              <CardContent className="p-3 grid grid-cols-2 sm:grid-cols-3 gap-x-4 gap-y-2 text-xs">
                <InfoRow label={t("detail.fields.type")} value={wo.type_label ?? "—"} />
                <InfoRow label={t("detail.fields.equipment")} value={wo.asset_label ?? "—"} />
                <InfoRow
                  label={t("detail.fields.assignedTo")}
                  value={wo.responsible_username ?? "—"}
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
                {wo.source_di_id && (
                  <InfoRow
                    label={String(t("diPanel.title")).split(" ")[0] || "DI"}
                    value={<span className="font-mono">DI-{wo.source_di_id}</span>}
                  />
                )}
              </CardContent>
            </Card>

            {/* Cost summary */}
            <WoCostSummaryCard woId={wo.id} status={wo.status_code ?? "draft"} />

            {/* Tabs */}
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
                  <WoExecutionControls wo={wo} canEdit={canEditWo} />
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
                <WoAttachmentPanel woId={wo.id} canUpload={canEditWo} canDelete={canEditWo} />
              </TabsContent>
            </Tabs>
          </div>

          {/* ── Footer ──────────────────────────────────────────────── */}
          <Separator />
          <div className="flex items-center justify-between gap-2 px-6 py-3">
            <div className="flex items-center gap-2">
              <FooterActions
                wo={wo}
                saving={saving}
                can={can}
                t={t}
                onEdit={() => {
                  openCreateForm(wo);
                  onClose();
                }}
                onSwitchToPlanning={() => setActiveTab("plan")}
                onSwitchToExecution={() => setActiveTab("execution")}
                onSwitchToCloseout={() => setActiveTab("closeout")}
                onComplete={openCompletionDialog}
                onClose={() => {
                  if (!info?.user_id) return;
                  void closeWorkOrder({
                    wo_id: wo.id,
                    actor_id: info.user_id,
                    expected_row_version: wo.row_version,
                  });
                }}
              />
            </div>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm" onClick={handlePrint} className="gap-1.5">
                <Printer className="h-3.5 w-3.5" />
                {t("print.button")}
              </Button>
              <Button variant="outline" size="sm" onClick={onClose} className="gap-1.5">
                <X className="h-3.5 w-3.5" />
                {t("detail.close")}
              </Button>
            </div>
          </div>
        </DialogContent>
      </Dialog>

      {/* Completion dialog (overlay on top of detail) */}
      <WoCompletionDialog wo={wo} />
    </>
  );
}

// ── Footer action buttons ───────────────────────────────────────────────────

interface FooterActionsProps {
  wo: WorkOrder;
  saving: boolean;
  can: (p: string) => boolean;
  t: (key: string) => string;
  onEdit: () => void;
  onSwitchToPlanning: () => void;
  onSwitchToExecution: () => void;
  onSwitchToCloseout: () => void;
  onComplete: () => void;
  onClose: () => void;
}

function FooterActions({
  wo,
  saving,
  can,
  t,
  onEdit,
  onSwitchToPlanning,
  onSwitchToExecution,
  onSwitchToCloseout,
  onComplete,
  onClose,
}: FooterActionsProps) {
  const s = (wo.status_code ?? "") as WoStatus;

  return (
    <>
      {/* draft → Edit */}
      {s === "draft" && can("ot.edit") && (
        <Button size="sm" variant="outline" onClick={onEdit} disabled={saving} className="gap-1.5">
          <Pencil className="h-3.5 w-3.5" />
          {t("action.edit")}
        </Button>
      )}

      {/* draft / planned → Schedule (opens Plan tab) */}
      {(s === "draft" || s === "planned") && can("ot.plan") && (
        <Button
          size="sm"
          variant="outline"
          onClick={onSwitchToPlanning}
          disabled={saving}
          className="gap-1.5"
        >
          <Settings className="h-3.5 w-3.5" />
          {t("footer.schedule")}
        </Button>
      )}

      {/* ready_to_schedule → Assign (opens Plan tab — assignment section is there) */}
      {s === "ready_to_schedule" && can("ot.assign") && (
        <Button
          size="sm"
          variant="outline"
          onClick={onSwitchToPlanning}
          disabled={saving}
          className="gap-1.5"
        >
          <FileText className="h-3.5 w-3.5" />
          {t("footer.assign")}
        </Button>
      )}

      {/* assigned → Start + re-Assign (opens Execution tab) */}
      {(s as string) === "assigned" && (
        <>
          {can("ot.execute") && (
            <Button
              size="sm"
              onClick={onSwitchToExecution}
              disabled={saving}
              className="gap-1.5 bg-green-600 hover:bg-green-700 text-white"
            >
              <Play className="h-3.5 w-3.5" />
              {t("action.start")}
            </Button>
          )}
          {can("ot.assign") && (
            <Button
              size="sm"
              variant="outline"
              onClick={onSwitchToPlanning}
              disabled={saving}
              className="gap-1.5"
            >
              <FileText className="h-3.5 w-3.5" />
              {t("footer.assign")}
            </Button>
          )}
        </>
      )}

      {/* in_progress → Pause (opens Execution tab) + Complete */}
      {s === "in_progress" && can("ot.execute") && (
        <>
          <Button
            size="sm"
            variant="outline"
            onClick={onSwitchToExecution}
            disabled={saving}
            className="gap-1.5"
          >
            <Pause className="h-3.5 w-3.5" />
            {t("action.pause")}
          </Button>
          <Button
            size="sm"
            onClick={onComplete}
            disabled={saving}
            className="gap-1.5 bg-amber-600 hover:bg-amber-700 text-white"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            {t("action.complete")}
          </Button>
        </>
      )}

      {/* paused / waiting_for_prerequisite → Resume (opens Execution tab) */}
      {(s === "paused" || (s as string) === "waiting_for_prerequisite") && can("ot.execute") && (
        <Button
          size="sm"
          onClick={onSwitchToExecution}
          disabled={saving}
          className="gap-1.5 bg-blue-600 hover:bg-blue-700 text-white"
        >
          <Play className="h-3.5 w-3.5" />
          {t("footer.resume")}
        </Button>
      )}

      {/* mechanically_complete → Verify (opens closeout tab) + Close */}
      {(s as string) === "mechanically_complete" && (
        <>
          {can("ot.verify") && (
            <Button
              size="sm"
              variant="outline"
              onClick={onSwitchToCloseout}
              disabled={saving}
              className="gap-1.5"
            >
              <ClipboardCheck className="h-3.5 w-3.5" />
              {t("action.verify")}
            </Button>
          )}
          {can("ot.close") && (
            <Button size="sm" onClick={onClose} disabled={saving} className="gap-1.5">
              {t("action.close")}
            </Button>
          )}
        </>
      )}

      {/* technically_verified → Close */}
      {(s as string) === "technically_verified" && can("ot.close") && (
        <Button size="sm" onClick={onClose} disabled={saving} className="gap-1.5">
          {t("action.close")}
        </Button>
      )}
    </>
  );
}

// ── Sub-components ──────────────────────────────────────────────────────────

function InfoRow({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="text-muted-foreground">{label}:</span>
      <span className="font-medium">{value}</span>
    </div>
  );
}
