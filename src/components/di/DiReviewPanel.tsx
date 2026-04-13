/**
 * DiReviewPanel.tsx
 *
 * Approver-facing collapsible panel showing a sorted queue of pending DIs.
 * Visible only to users with `di.review` permission.
 * Sort: priority desc → submitted_at asc → equipment → requester.
 *
 * Phase 2 – Sub-phase 04 – File 02 – Sprint S4.
 */

import { Check, ChevronDown, ChevronRight, ClipboardCheck, Eye, RotateCcw, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import { toErrorMessage } from "@/utils/errors";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Priority ordering ───────────────────────────────────────────────────────

const URGENCY_ORDER: Record<string, number> = {
  critical: 4,
  high: 3,
  medium: 2,
  low: 1,
};

const URGENCY_STYLE: Record<string, string> = {
  critical: "bg-red-100 text-red-700",
  high: "bg-orange-100 text-orange-800",
  medium: "bg-yellow-100 text-yellow-800",
  low: "bg-green-100 text-green-800",
};

// ── Helpers ─────────────────────────────────────────────────────────────────

function formatShortDate(iso: string): string {
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

function sortReviewQueue(items: InterventionRequest[]): InterventionRequest[] {
  return [...items].sort((a, b) => {
    // 1. priority desc
    const pa = URGENCY_ORDER[a.reported_urgency] ?? 0;
    const pb = URGENCY_ORDER[b.reported_urgency] ?? 0;
    if (pa !== pb) return pb - pa;
    // 2. submitted_at asc
    if (a.submitted_at !== b.submitted_at) return a.submitted_at.localeCompare(b.submitted_at);
    // 3. equipment (asset_id) asc
    if (a.asset_id !== b.asset_id) return a.asset_id - b.asset_id;
    // 4. submitter_id asc
    return a.submitter_id - b.submitter_id;
  });
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiReviewPanel() {
  const { t } = useTranslation("di");
  const reviewQueue = useDiReviewStore((s) => s.reviewQueue);
  const loadReviewQueue = useDiReviewStore((s) => s.loadReviewQueue);
  const reviewError = useDiReviewStore((s) => s.error);
  const screen = useDiReviewStore((s) => s.screen);
  const openApproval = useDiReviewStore((s) => s.openApproval);
  const openRejection = useDiReviewStore((s) => s.openRejection);
  const openReturn = useDiReviewStore((s) => s.openReturn);
  const openDi = useDiStore((s) => s.openDi);

  const [collapsed, setCollapsed] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  useEffect(() => {
    void loadReviewQueue();
  }, [loadReviewQueue]);

  const sorted = useMemo(() => sortReviewQueue(reviewQueue), [reviewQueue]);

  const handleView = useCallback(
    (di: InterventionRequest) => {
      void openDi(di.id);
    },
    [openDi],
  );

  const handleQuickScreen = useCallback(
    async (di: InterventionRequest) => {
      setLocalError(null);
      try {
        const updated = await screen({
          di_id: di.id,
          actor_id: 0, // overridden by backend
          expected_row_version: di.row_version,
          validated_urgency: di.reported_urgency,
          classification_code_id: di.classification_code_id ?? di.symptom_code_id ?? null,
          reviewer_note: null,
        });
        // Once triage succeeds and DI is awaiting_approval, open approval dialog directly.
        if (updated.status === "awaiting_approval") {
          openApproval(updated);
        }
      } catch (err) {
        console.error("[DiReviewPanel] quick screen failed:", err);
        setLocalError(toErrorMessage(err));
      }
    },
    [screen, openApproval],
  );

  if (sorted.length === 0) return null;

  return (
    <Card className="mx-4 mt-4 border-amber-200 bg-amber-50/30">
      {/* Header */}
      <button
        type="button"
        className="flex w-full items-center gap-2 px-4 py-3 text-left text-sm font-medium"
        onClick={() => setCollapsed((c) => !c)}
      >
        {collapsed ? (
          <ChevronRight className="h-4 w-4 shrink-0" />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0" />
        )}
        <span>{t("review.panelTitle")}</span>
        <Badge variant="secondary" className="text-[10px] h-5 min-w-[20px] justify-center">
          {sorted.length}
        </Badge>
      </button>

      {/* Queue rows */}
      {!collapsed && (
        <CardContent className="p-0">
          {(localError ?? reviewError) && (
            <div
              role="alert"
              className="mx-4 mt-3 rounded-md bg-destructive/10 px-3 py-2 text-xs text-destructive"
            >
              {localError ?? reviewError}
            </div>
          )}
          <div className="divide-y">
            {sorted.map((di) => (
              <ReviewRow
                key={di.id}
                di={di}
                onScreen={() => handleQuickScreen(di)}
                onApprove={() => openApproval(di)}
                onReject={() => openRejection(di)}
                onReturn={() => openReturn(di)}
                onView={() => handleView(di)}
              />
            ))}
          </div>
        </CardContent>
      )}
    </Card>
  );
}

// ── Row ─────────────────────────────────────────────────────────────────────

function ReviewRow({
  di,
  onScreen,
  onApprove,
  onReject,
  onReturn,
  onView,
}: {
  di: InterventionRequest;
  onScreen: () => void;
  onApprove: () => void;
  onReject: () => void;
  onReturn: () => void;
  onView: () => void;
}) {
  const { t } = useTranslation("di");

  return (
    <div className="flex items-center gap-3 px-4 py-2.5 text-xs hover:bg-muted/50">
      {/* Code + modified badge */}
      <span className="font-mono text-muted-foreground shrink-0 w-[80px]">
        {di.code}
        {di.is_modified && (
          <Badge className="ml-1.5 bg-amber-100 text-amber-800 border-0 text-[9px] px-1 py-0">
            {t("review.modified")}
          </Badge>
        )}
      </span>

      {/* Title */}
      <span className="truncate min-w-0 flex-1 font-medium">{di.title}</span>

      {/* Priority */}
      <Badge
        variant="outline"
        className={`text-[10px] border-0 shrink-0 ${URGENCY_STYLE[di.reported_urgency] ?? ""}`}
      >
        {t(`priority.${di.reported_urgency}`)}
      </Badge>

      {/* Equipment */}
      <span className="text-muted-foreground shrink-0 w-[60px] text-right">#{di.asset_id}</span>

      {/* Requester */}
      <span className="text-muted-foreground shrink-0 w-[60px] text-right">#{di.submitter_id}</span>

      {/* Submitted date */}
      <span className="text-muted-foreground shrink-0 w-[80px] text-right">
        {formatShortDate(di.submitted_at)}
      </span>

      {/* Actions */}
      <div className="flex items-center gap-1 shrink-0">
        {di.status === "awaiting_approval" && (
          <Button
            variant="ghost"
            size="sm"
            className="h-6 w-6 p-0 text-green-600 hover:text-green-700 hover:bg-green-50"
            onClick={onApprove}
            title={t("action.approve")}
          >
            <Check className="h-3.5 w-3.5" />
          </Button>
        )}
        {(di.status === "pending_review" || di.status === "returned_for_clarification") && (
          <Button
            variant="ghost"
            size="sm"
            className="h-6 w-6 p-0 text-blue-600 hover:text-blue-700 hover:bg-blue-50"
            onClick={onScreen}
            title={t("review.screenAction", "Valider le tri")}
          >
            <ClipboardCheck className="h-3.5 w-3.5" />
          </Button>
        )}
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0 text-red-600 hover:text-red-700 hover:bg-red-50"
          onClick={onReject}
          title={t("action.reject")}
        >
          <X className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0 text-amber-600 hover:text-amber-700 hover:bg-amber-50"
          onClick={onReturn}
          title={t("review.returnAction")}
        >
          <RotateCcw className="h-3.5 w-3.5" />
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          onClick={onView}
          title={t("review.view")}
        >
          <Eye className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  );
}
