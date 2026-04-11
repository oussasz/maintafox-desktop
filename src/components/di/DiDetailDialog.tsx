/**
 * DiDetailDialog.tsx
 *
 * Floating detail window for a single intervention request.
 * Renders as a centered modal dialog (not a side panel) so the underlying
 * Kanban / list view stays fully visible and interactive when closed.
 *
 * UX pattern: "Entity Detail Dialog" — the unified desktop detail view
 * pattern described in PRD §UX-DW-001. All entity detail views (DI, WO,
 * Equipment, etc.) should follow this pattern:
 *   • Centered modal overlay with `max-w-3xl` / `max-h-[85vh]`
 *   • Header: entity code + title + status badge + close button
 *   • Body: scrollable info section + tabbed sub-panels
 *   • Footer: contextual action buttons (approve, reject, etc.)
 *
 * Phase 2 – Sub-phase 04 – Sprint S4.
 */

import { Calendar, Printer, Shield, User, X } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { DiDetailPanel } from "@/components/di/DiDetailPanel";
import { printDiFiche } from "@/components/di/DiPrintFiche";
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
import type { InterventionRequest } from "@shared/ipc-types";

// ── Status → badge style mapping ────────────────────────────────────────────

const STATUS_STYLE: Record<string, string> = {
  submitted: "bg-blue-100 text-blue-800",
  pending_review: "bg-amber-100 text-amber-800",
  returned_for_clarification: "bg-orange-100 text-orange-800",
  rejected: "bg-red-100 text-red-700",
  screened: "bg-sky-100 text-sky-800",
  awaiting_approval: "bg-yellow-100 text-yellow-800",
  approved_for_planning: "bg-green-100 text-green-800",
  deferred: "bg-gray-100 text-gray-600",
  converted_to_work_order: "bg-emerald-100 text-emerald-800",
  closed_as_non_executable: "bg-slate-100 text-slate-600",
  archived: "bg-neutral-100 text-neutral-500",
};

const URGENCY_STYLE: Record<string, string> = {
  low: "bg-green-100 text-green-800",
  medium: "bg-yellow-100 text-yellow-800",
  high: "bg-orange-100 text-orange-800",
  critical: "bg-red-100 text-red-700",
};

type DiStatusKey =
  | "new"
  | "inReview"
  | "approved"
  | "rejected"
  | "inProgress"
  | "resolved"
  | "closed"
  | "cancelled";

function statusToI18nKey(s: string): DiStatusKey {
  const map: Record<string, DiStatusKey> = {
    submitted: "new",
    pending_review: "inReview",
    returned_for_clarification: "inReview",
    rejected: "rejected",
    screened: "inReview",
    awaiting_approval: "inReview",
    approved_for_planning: "approved",
    deferred: "inReview",
    converted_to_work_order: "inProgress",
    closed_as_non_executable: "closed",
    archived: "closed",
  };
  return map[s] ?? "new";
}

function formatDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString("fr-FR", {
      day: "2-digit",
      month: "2-digit",
      year: "numeric",
    });
  } catch {
    return iso;
  }
}

// ── Props ───────────────────────────────────────────────────────────────────

interface DiDetailDialogProps {
  di: InterventionRequest | null;
  open: boolean;
  onClose: () => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiDetailDialog({ di, open, onClose }: DiDetailDialogProps) {
  const { t } = useTranslation("di");

  if (!di) return null;

  const statusKey = statusToI18nKey(di.status);

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <DialogContent
        className="max-w-3xl max-h-[85vh] flex flex-col p-0 gap-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        {/* ── Header ──────────────────────────────────────────────────── */}
        <DialogHeader className="px-6 pt-5 pb-3">
          <div className="flex items-start justify-between gap-4">
            <div className="space-y-1 min-w-0">
              <DialogTitle className="text-lg font-bold flex items-center gap-2">
                <span className="text-muted-foreground font-mono text-base">{di.code}</span>
                {di.is_modified && (
                  <Badge className="bg-amber-100 text-amber-800 border-0 text-[10px] px-1.5 py-0">
                    {t("review.modified")}
                  </Badge>
                )}
                <span className="truncate">{di.title}</span>
              </DialogTitle>
              <DialogDescription className="text-sm text-muted-foreground line-clamp-2">
                {di.description}
              </DialogDescription>
            </div>
            <div className="flex items-center gap-1.5 shrink-0 pt-0.5">
              <Badge
                variant="outline"
                className={`text-xs border-0 ${STATUS_STYLE[di.status] ?? "bg-gray-100"}`}
              >
                {t(`status.${statusKey}` as const)}
              </Badge>
              {di.safety_flag && (
                <Badge variant="destructive" className="text-xs gap-1">
                  <Shield className="h-3 w-3" />
                  {t("detail.safety")}
                </Badge>
              )}
            </div>
          </div>
        </DialogHeader>

        <Separator />

        {/* ── Scrollable body ─────────────────────────────────────────── */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Info grid */}
          <Card>
            <CardContent className="p-3 grid grid-cols-2 sm:grid-cols-3 gap-x-4 gap-y-2 text-xs">
              <InfoRow
                icon={<Calendar className="h-3 w-3" />}
                label={t("detail.fields.reportedAt")}
                value={formatDate(di.submitted_at)}
              />
              <InfoRow
                icon={<User className="h-3 w-3" />}
                label={t("detail.fields.reportedBy")}
                value={`#${di.submitter_id}`}
              />
              <InfoRow
                label={t("detail.fields.priority")}
                value={
                  <Badge
                    variant="outline"
                    className={`text-[10px] border-0 ${URGENCY_STYLE[di.reported_urgency] ?? ""}`}
                  >
                    {t(`priority.${di.reported_urgency}`)}
                  </Badge>
                }
              />
              <InfoRow label={t("detail.fields.origin")} value={di.origin_type} />
              <InfoRow label={t("detail.fields.asset")} value={`#${di.asset_id}`} />
              <InfoRow label={t("detail.fields.orgNode")} value={`#${di.org_node_id}`} />
              {di.production_impact && (
                <InfoRow label={t("detail.fields.impact")} value={t("detail.fields.production")} />
              )}
            </CardContent>
          </Card>

          {/* Tabs: attachments + audit */}
          <DiDetailPanel di={di} canUploadAttachment={true} canDeleteAttachment={true} />
        </div>

        {/* ── Footer ──────────────────────────────────────────────────── */}
        <Separator />
        <div className="flex items-center justify-end gap-2 px-6 py-3">
          <Button variant="outline" size="sm" onClick={() => printDiFiche(di)} className="gap-1.5">
            <Printer className="h-3.5 w-3.5" />
            {t("review.print")}
          </Button>
          <Button variant="outline" size="sm" onClick={onClose} className="gap-1.5">
            <X className="h-3.5 w-3.5" />
            {t("detail.close")}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ── Sub-components ──────────────────────────────────────────────────────────

function InfoRow({ icon, label, value }: { icon?: ReactNode; label: string; value: ReactNode }) {
  return (
    <div className="flex items-center gap-1.5">
      {icon}
      <span className="text-muted-foreground">{label}:</span>
      <span className="font-medium">{value}</span>
    </div>
  );
}
