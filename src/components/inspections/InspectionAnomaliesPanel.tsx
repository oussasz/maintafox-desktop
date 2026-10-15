/**
 * Anomaly follow-up list across rounds (PRD §6.25): what was found, on which equipment, the
 * reading, and the decision — a request or a work order (linked, openable) or a deferral.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { FileWarning, Hammer, PauseCircle } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { LinkedEntityBadge } from "@/components/common/LinkedEntityBadge";
import { DataTable } from "@/components/data/DataTable";
import { Button } from "@/components/ui";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, TruncationNotice } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { deferInspectionAnomaly, listInspectionAnomalyRows } from "@/services/inspection-service";
import type { InspectionAnomalyRow } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import {
  type FollowUpCreated,
  FollowUpCreatedNotice,
  type FollowUpKind,
  InspectionFollowUpDialog,
} from "./InspectionFollowUpDialog";
import { ANOMALY_STATUS_TONE, RESULT_TONE } from "./inspection-rules";

/** Anomalies loaded per page (backend maximum: `insights::MAX_ANOMALY_PAGE`). */
const PAGE_SIZE = 200;

interface InspectionAnomaliesPanelProps {
  /** Resolution status to show, or null for every status. */
  status: string | null;
  refreshKey: number;
  onChanged: () => void;
  onOpenRound: (roundId: number) => void;
  onCountChange?: (total: number | null) => void;
}

export function InspectionAnomaliesPanel({
  status,
  refreshKey,
  onChanged,
  onOpenRound,
  onCountChange,
}: InspectionAnomaliesPanelProps) {
  const { t } = useTranslation("inspections");
  const { can, canAny } = usePermissions();
  const { formatNumber, formatDateTime } = useFormatters();
  const [rows, setRows] = useState<InspectionAnomalyRow[]>([]);
  const [total, setTotal] = useState(0);
  const [loading, setLoading] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [followUp, setFollowUp] = useState<{ id: number; kind: FollowUpKind } | null>(null);
  const [created, setCreated] = useState<FollowUpCreated | null>(null);
  const [deferTarget, setDeferTarget] = useState<InspectionAnomalyRow | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const page = await listInspectionAnomalyRows({
        statuses: status ? [status] : null,
        limit: PAGE_SIZE,
        offset: 0,
      });
      setRows(page.items);
      setTotal(page.total);
      setError(null);
      onCountChange?.(page.total);
    } catch (err) {
      setError(err);
      onCountChange?.(null);
    } finally {
      setLoading(false);
    }
  }, [status, onCountChange]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const loadMore = async () => {
    setLoadingMore(true);
    try {
      const page = await listInspectionAnomalyRows({
        statuses: status ? [status] : null,
        limit: PAGE_SIZE,
        offset: rows.length,
      });
      setRows((prev) => [...prev, ...page.items]);
      setTotal(page.total);
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setLoadingMore(false);
    }
  };

  const canDecide = can(P.INS_EXECUTE);
  const canDi = canDecide && canAny(P.DI_CREATE, P.DI_CREATE_OWN);
  const canWo = canDecide && can(P.OT_CREATE);

  const columns = useMemo<ColumnDef<InspectionAnomalyRow>[]>(
    () => [
      {
        id: "finding",
        accessorFn: (r) => r.checkpoint_code ?? "",
        header: t("anomalies.columns.finding"),
        cell: ({ row }) => {
          const r = row.original;
          return (
            <div className="min-w-0 space-y-0.5">
              <p className="flex items-center gap-1.5 font-medium text-text-primary">
                <FileWarning className="h-4 w-4 shrink-0 text-tone-attention-fg" aria-hidden />
                {t(`review.anomalyType.${r.anomaly.anomaly_type}`, {
                  defaultValue: t("anomalies.otherType"),
                })}
              </p>
              <p className="text-xs text-text-muted">
                <span className="font-mono">{r.checkpoint_code}</span>
                {r.checkpoint_description ? ` — ${r.checkpoint_description}` : ""}
              </p>
            </div>
          );
        },
      },
      {
        id: "equipment",
        accessorFn: (r) => r.asset_code ?? "",
        header: t("anomalies.columns.equipment"),
        cell: ({ row }) =>
          row.original.asset_code ? (
            <LinkedEntityBadge
              entity="asset"
              code={row.original.asset_code}
              entityId={row.original.asset_id}
              title={row.original.asset_name}
            />
          ) : (
            <span className="text-xs text-text-muted">—</span>
          ),
      },
      {
        id: "reading",
        accessorFn: (r) => r.numeric_value ?? 0,
        header: t("anomalies.columns.reading"),
        cell: ({ row }) => {
          const r = row.original;
          return (
            <div className="flex flex-wrap items-center gap-1.5 text-xs">
              {r.numeric_value != null ? (
                <span className="font-mono">
                  {formatNumber(r.numeric_value)}
                  {r.measurement_unit ? ` ${r.measurement_unit}` : ""}
                </span>
              ) : null}
              {r.result_status ? (
                <StatusChip
                  label={t(`result.${r.result_status}`)}
                  tone={toneOf(RESULT_TONE, r.result_status)}
                />
              ) : null}
            </div>
          );
        },
      },
      {
        id: "round",
        accessorFn: (r) => r.round_scheduled_at ?? "",
        header: t("anomalies.columns.round"),
        cell: ({ row }) => (
          <Button
            variant="link"
            className="h-11 px-0 text-left text-xs"
            onClick={(e) => {
              e.stopPropagation();
              onOpenRound(row.original.anomaly.round_id);
            }}
          >
            {row.original.template_name} · {formatDateTime(row.original.round_scheduled_at)}
          </Button>
        ),
      },
      {
        id: "status",
        accessorFn: (r) => r.anomaly.resolution_status,
        header: t("anomalies.columns.status"),
        cell: ({ row }) => (
          <StatusChip
            label={t(`anomalyStatus.${row.original.anomaly.resolution_status}`)}
            tone={toneOf(ANOMALY_STATUS_TONE, row.original.anomaly.resolution_status)}
            dot
          />
        ),
      },
      {
        id: "followUp",
        enableSorting: false,
        header: t("anomalies.columns.followUp"),
        cell: ({ row }) => {
          const r = row.original;
          if (r.linked_di_code || r.linked_wo_code) {
            return (
              <div className="flex flex-wrap gap-1">
                <LinkedEntityBadge
                  entity="di"
                  code={r.linked_di_code}
                  entityId={r.anomaly.linked_di_id}
                  title={r.linked_di_title}
                />
                <LinkedEntityBadge
                  entity="work_order"
                  code={r.linked_wo_code}
                  entityId={r.anomaly.linked_work_order_id}
                  title={r.linked_wo_title}
                />
              </div>
            );
          }
          if (r.anomaly.resolution_status !== "open" || !canDecide) {
            return <span className="text-xs text-text-muted">—</span>;
          }
          return (
            <div className="flex flex-wrap gap-1.5">
              {canDi ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="h-11"
                  onClick={(e) => {
                    e.stopPropagation();
                    setCreated(null);
                    setFollowUp({ id: r.anomaly.id, kind: "di" });
                  }}
                >
                  {t("review.toDi")}
                </Button>
              ) : null}
              {canWo ? (
                <Button
                  size="sm"
                  variant="outline"
                  className="h-11"
                  onClick={(e) => {
                    e.stopPropagation();
                    setCreated(null);
                    setFollowUp({ id: r.anomaly.id, kind: "wo" });
                  }}
                >
                  <Hammer className="mr-1.5 h-4 w-4" aria-hidden />
                  {t("review.toWo")}
                </Button>
              ) : null}
              <Button
                size="sm"
                variant="ghost"
                className="h-11"
                onClick={(e) => {
                  e.stopPropagation();
                  setDeferTarget(r);
                }}
              >
                <PauseCircle className="mr-1.5 h-4 w-4" aria-hidden />
                {t("review.defer")}
              </Button>
            </div>
          );
        },
      },
    ],
    [canDecide, canDi, canWo, formatDateTime, formatNumber, onOpenRound, t],
  );

  const confirmDefer = async () => {
    const target = deferTarget;
    if (!target) return;
    // A failure keeps the dialog open (ConfirmDialog) and is reported by the global IPC toast.
    await deferInspectionAnomaly({
      anomaly_id: target.anomaly.id,
      expected_row_version: target.anomaly.row_version,
    });
    onChanged();
    await load();
  };

  const body =
    error != null && rows.length === 0 ? (
      <ErrorState error={error} onRetry={() => void load()} retrying={loading} />
    ) : !loading && rows.length === 0 ? (
      <EmptyState title={t("anomalies.empty")} description={t("anomalies.emptyHint")} />
    ) : (
      <>
        <TruncationNotice
          shown={rows.length}
          total={total}
          onLoadMore={() => void loadMore()}
          loadingMore={loadingMore}
        />
        <DataTable
          columns={columns}
          data={rows}
          pageSize={20}
          isLoading={loading}
          skeletonRows={6}
        />
      </>
    );

  return (
    <div className="space-y-3" data-testid="inspection-anomaly-list">
      {created ? <FollowUpCreatedNotice created={created} /> : null}
      {body}
      <InspectionFollowUpDialog
        anomalyId={followUp?.id ?? null}
        kind={followUp?.kind ?? "di"}
        onClose={() => setFollowUp(null)}
        onCreated={(c) => {
          setFollowUp(null);
          setCreated(c);
          onChanged();
          void load();
        }}
      />
      <ConfirmDialog
        open={deferTarget != null}
        onOpenChange={(open) => (!open ? setDeferTarget(null) : undefined)}
        title={t("anomalies.deferTitle")}
        description={t("anomalies.deferBody", {
          checkpoint: deferTarget?.checkpoint_code ?? "",
        })}
        confirmLabel={t("review.defer")}
        onConfirm={confirmDefer}
      />
    </div>
  );
}
