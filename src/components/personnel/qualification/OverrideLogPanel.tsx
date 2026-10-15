/**
 * Qualification overrides: every time a supervisor let someone work without a required
 * qualification — who, on which job, what was missing, why, approved by whom and when.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { ShieldAlert } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { DataTable } from "@/components/data/DataTable";
import { ErrorState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import {
  type QualificationOverrideRow,
  listQualificationOverrides,
} from "@/services/qualification-gate-service";

import { describeGap } from "./QualificationCheckNotice";
import { GAP_STATUS_TONE } from "./qualification-tones";

export function OverrideLogPanel({ refreshKey = 0 }: { refreshKey?: number }) {
  const { t, i18n } = useTranslation("training");
  const [rows, setRows] = useState<QualificationOverrideRow[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const fmt = useMemo(
    () => new Intl.DateTimeFormat(i18n.language, { dateStyle: "medium", timeStyle: "short" }),
    [i18n.language],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setRows(await listQualificationOverrides({ limit: 500 }));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const columns = useMemo<ColumnDef<QualificationOverrideRow>[]>(
    () => [
      {
        accessorKey: "approved_at",
        header: t("col.when"),
        cell: ({ row }) => (
          <span className="tabular-nums">{fmt.format(new Date(row.original.approved_at))}</span>
        ),
      },
      { accessorKey: "person_name", header: t("col.person") },
      {
        id: "job",
        header: t("col.job"),
        cell: ({ row }) =>
          row.original.context_kind === "work_order"
            ? t("overrides.onWorkOrder", { code: row.original.work_order_code ?? "" })
            : t("overrides.onPermitRole", {
                code: row.original.permit_code ?? "",
                role: t(`lotoRole.${row.original.role_code}`, {
                  defaultValue: row.original.role_code ?? "",
                }),
              }),
      },
      {
        id: "gaps",
        header: t("col.missing"),
        cell: ({ row }) => (
          <ul className="space-y-1">
            {row.original.gaps.map((g) => (
              <li key={g.requirement_key} className="flex items-center gap-1.5">
                <StatusChip
                  label={t(`gapStatus.${g.status}`)}
                  tone={GAP_STATUS_TONE[g.status]}
                  dot
                />
                <span className="text-xs">{describeGap(t, g)}</span>
              </li>
            ))}
          </ul>
        ),
      },
      { accessorKey: "reason", header: t("col.reason") },
      {
        accessorKey: "approved_by_name",
        header: t("col.approvedBy"),
        cell: ({ row }) => row.original.approved_by_name ?? "—",
      },
    ],
    [t, fmt],
  );

  if (error && rows.length === 0)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;

  return (
    <div className="space-y-3">
      <p className="text-sm text-text-muted">{t("overrides.intro")}</p>
      {!loading && rows.length === 0 ? (
        <EmptyState
          icon={<ShieldAlert className="h-8 w-8" aria-hidden />}
          title={t("overrides.empty")}
        />
      ) : (
        <DataTable
          columns={columns}
          data={rows}
          pageSize={20}
          isLoading={loading && rows.length === 0}
        />
      )}
    </div>
  );
}
