/**
 * "Équipement déclassé — à revoir" (AU-73b): work orders and requests open on retired equipment,
 * crossed in sync with the retirement (backend `wo::retired_equipment_review`).
 *
 * Read only. Each item is settled with the existing actions: cancel the work order or reject the
 * request with a reason (opened from here), or return the equipment to service. The open items
 * are read on mount and again whenever the work-order list reloads (after an action on the page).
 * A closed item keeps who, when and how: the history (bounded on the backend) is read only when it
 * is opened. With nothing open the card is hidden; a small link keeps the history reachable.
 *
 * On the Requests page (`requestsOnly`, AU-112) it lists the requests alone, needs only di.view,
 * opens a request in place and reads again when that page's list reloads (`reloadSignal`).
 */

import type { TFunction } from "i18next";
import { AlertTriangle, ChevronDown, ChevronRight, Eye } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent } from "@/components/ui/card";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type RetiredEquipmentReview,
  listRetiredEquipmentReviews,
} from "@/services/retired-equipment-review-service";
import { useWoStore } from "@/stores/wo-store";

function closureText(t: TFunction<"ot">, item: RetiredEquipmentReview): string {
  return t(`retiredEquipment.closure.${item.closure_kind ?? "other"}`, {
    code: item.record_code ?? "—",
    wo: item.converted_to_wo_code ?? "—",
    equipment: item.equipment_code ?? "—",
  });
}

function History({ items }: { items: RetiredEquipmentReview[] }) {
  const { t } = useTranslation("ot");
  const { formatDateTime } = useFormatters();
  return (
    <ul className="mt-1 divide-y rounded-md border bg-background text-xs">
      {items.map((item) => (
        <li key={item.id} className="px-3 py-2">
          <div className="font-medium">{closureText(t, item)}</div>
          <div className="text-muted-foreground">
            {item.closed_by
              ? t("retiredEquipment.closedBy", {
                  who: item.closed_by,
                  when: formatDateTime(item.closed_at),
                })
              : t("retiredEquipment.closedAt", { when: formatDateTime(item.closed_at) })}
            {item.closure_note
              ? ` · ${t("retiredEquipment.note", { note: item.closure_note })}`
              : ""}
          </div>
        </li>
      ))}
    </ul>
  );
}

interface WoRetiredEquipmentReviewPanelProps {
  /** The Requests page: requests only (di.view), opened with `onOpenRequest`. */
  requestsOnly?: boolean;
  onOpenRequest?: (diId: number) => void;
  /** Changes when the host page's list reloads; defaults to the work-order list. */
  reloadSignal?: unknown;
}

export function WoRetiredEquipmentReviewPanel({
  requestsOnly = false,
  onOpenRequest,
  reloadSignal: hostReloadSignal,
}: WoRetiredEquipmentReviewPanelProps = {}) {
  const { t } = useTranslation("ot");
  const openWo = useWoStore((s) => s.openWo);
  // The work-order list reloads after every action on the page: read the open items again then.
  const woItems = useWoStore((s) => s.items);
  const reloadSignal = requestsOnly ? hostReloadSignal : woItems;
  const [open, setOpen] = useState<RetiredEquipmentReview[]>([]);
  const [closedCount, setClosedCount] = useState(0);
  const [history, setHistory] = useState<RetiredEquipmentReview[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [collapsed, setCollapsed] = useState(false);
  const [showHistory, setShowHistory] = useState(false);

  // An open history stays open across reloads and is read again with them.
  const historyShown = useRef(false);
  historyShown.current = showHistory;

  const load = useCallback(async () => {
    try {
      const withHistory = historyShown.current;
      const list = await listRetiredEquipmentReviews(withHistory, requestsOnly);
      setOpen(list.items.filter((i) => i.status === "open"));
      setClosedCount(list.closed_count);
      setHistory(withHistory ? list.items.filter((i) => i.status === "closed") : null);
      setFailed(false);
    } catch {
      setFailed(true);
    }
  }, [requestsOnly]);

  useEffect(() => {
    void load();
  }, [load, reloadSignal]);

  const toggleHistory = useCallback(async () => {
    if (showHistory) {
      setShowHistory(false);
      return;
    }
    setShowHistory(true);
    if (history !== null) return;
    try {
      const list = await listRetiredEquipmentReviews(true, requestsOnly);
      setHistory(list.items.filter((i) => i.status === "closed"));
    } catch {
      setFailed(true);
    }
  }, [history, showHistory, requestsOnly]);

  if (failed) {
    return (
      <p
        role="alert"
        className="mx-4 mt-4 rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
      >
        {t("retiredEquipment.loadFailed")}
      </p>
    );
  }
  if (open.length === 0 && closedCount === 0) return null;

  const historyBlock =
    closedCount > 0 ? (
      <div>
        <button
          type="button"
          className="text-xs font-medium text-muted-foreground hover:underline"
          onClick={() => void toggleHistory()}
          aria-expanded={showHistory}
        >
          {open.length === 0
            ? t("retiredEquipment.historyLink", { count: closedCount })
            : t("retiredEquipment.history", { count: closedCount })}
        </button>
        {showHistory && history ? <History items={history} /> : null}
      </div>
    ) : null;

  if (open.length === 0) {
    return <div className="mx-4 mt-2">{historyBlock}</div>;
  }

  return (
    <Card
      className="mx-4 mt-4 border-tone-warning-border bg-tone-warning-bg"
      data-testid="retired-equipment-review"
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 px-4 py-3 text-left text-sm font-medium"
        onClick={() => setCollapsed((v) => !v)}
        aria-expanded={!collapsed}
      >
        {collapsed ? (
          <ChevronRight className="h-4 w-4 shrink-0" aria-hidden />
        ) : (
          <ChevronDown className="h-4 w-4 shrink-0" aria-hidden />
        )}
        <AlertTriangle className="h-4 w-4 shrink-0 text-tone-warning-fg" aria-hidden />
        <span>{t("retiredEquipment.title")}</span>
        <Badge variant="secondary" className="h-5 min-w-[20px] justify-center text-[10px]">
          {open.length}
        </Badge>
      </button>

      {!collapsed && (
        <CardContent className="space-y-2 px-4 pb-3 pt-0 text-xs">
          <p className="text-muted-foreground">{t("retiredEquipment.intro")}</p>
          <ul className="divide-y rounded-md border bg-background">
            {open.map((item) => (
              <li key={item.id} className="flex items-center gap-3 px-3 py-2">
                <Badge variant="outline" className="shrink-0 text-[10px]">
                  {t(`retiredEquipment.kind.${item.kind}`)}
                </Badge>
                <span className="w-[90px] shrink-0 font-mono text-muted-foreground">
                  {item.record_code ?? "—"}
                </span>
                <span className="min-w-0 flex-1 truncate font-medium">
                  {item.record_title ?? ""}
                </span>
                <span className="shrink-0 truncate text-muted-foreground">
                  {t("retiredEquipment.equipment", {
                    code: item.equipment_code ?? "—",
                    name: item.equipment_name ?? "",
                  })}
                </span>
                {item.record_id != null ? (
                  item.kind === "work_order" ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-6 w-6 shrink-0 p-0"
                      onClick={() => void openWo(item.record_id as number)}
                      title={t("retiredEquipment.open")}
                      aria-label={t("retiredEquipment.open")}
                    >
                      <Eye className="h-3.5 w-3.5" aria-hidden />
                    </Button>
                  ) : onOpenRequest ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      className="h-6 shrink-0 px-2 text-tone-warning-fg"
                      onClick={() => onOpenRequest(item.record_id as number)}
                    >
                      {t("retiredEquipment.open")}
                    </Button>
                  ) : (
                    <Link
                      to={`/requests?openDi=${item.record_id}`}
                      className="shrink-0 text-tone-warning-fg hover:underline"
                    >
                      {t("retiredEquipment.open")}
                    </Link>
                  )
                ) : null}
              </li>
            ))}
          </ul>
          {historyBlock}
        </CardContent>
      )}
    </Card>
  );
}
