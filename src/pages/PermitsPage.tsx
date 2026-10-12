import { HardHat } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { ModulePageShell } from "@/components/layout/ModulePageShell";
import {
  PermitCreateDialog,
  type PermitCreateWorkOrder,
} from "@/components/permits/PermitCreateDialog";
import { PermitDetailPanel } from "@/components/permits/PermitDetailPanel";
import { PermitTypesDialog } from "@/components/permits/PermitTypesDialog";
import {
  PERMIT_QUERY_CREATE_FOR_WO,
  PERMIT_QUERY_SELECT,
  formatAssetLabel,
} from "@/components/permits/permit-rules";
import { Badge, Button } from "@/components/ui";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type PermitComplianceKpi30dRecord,
  type PermitTypeRecord,
  type WorkPermitRecord,
  listOpenPermitsReport,
  listPermitTypes,
  listWorkPermits,
  permitComplianceKpi30d,
} from "@/services/permit-service";
import { getWo } from "@/services/wo-service";
import { toErrorMessage } from "@/utils/errors";
import { formatDateTime } from "@/utils/format-date";
import { P } from "@shared/rbac/permissions.generated";

export function PermitsPage() {
  const { t, i18n } = useTranslation("permits");
  const { can } = usePermissions();
  const canIssue = can(P.PTW_ISSUE);
  const [searchParams, setSearchParams] = useSearchParams();

  const [types, setTypes] = useState<PermitTypeRecord[]>([]);
  const [permits, setPermits] = useState<WorkPermitRecord[]>([]);
  const [openPermits, setOpenPermits] = useState<WorkPermitRecord[]>([]);
  const [kpi30d, setKpi30d] = useState<PermitComplianceKpi30dRecord | null>(null);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createWo, setCreateWo] = useState<PermitCreateWorkOrder | null>(null);
  const [typesOpen, setTypesOpen] = useState(false);

  const load = useCallback(async () => {
    setError(null);
    try {
      const [ty, p, kpi, open] = await Promise.all([
        listPermitTypes(),
        listWorkPermits({ limit: 200 }),
        permitComplianceKpi30d(),
        listOpenPermitsReport(),
      ]);
      setTypes(ty);
      setPermits(p);
      setKpi30d(kpi);
      setOpenPermits(open);
    } catch (e) {
      setError(toErrorMessage(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  // Deep links from the WO detail: select a permit or open "create" pre-linked to a WO.
  useEffect(() => {
    const selectParam = searchParams.get(PERMIT_QUERY_SELECT);
    const woParam = searchParams.get(PERMIT_QUERY_CREATE_FOR_WO);
    if (selectParam) setSelectedId(Number(selectParam));
    if (!woParam) return;
    let cancelled = false;
    void getWo(Number(woParam))
      .then(({ wo }) => {
        if (cancelled) return;
        const assetLabel =
          wo.asset_code && wo.asset_label
            ? `${wo.asset_code} — ${wo.asset_label}`
            : (wo.asset_code ?? wo.asset_label ?? null);
        setCreateWo({ id: wo.id, code: wo.code, assetLabel });
        setCreateOpen(true);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(toErrorMessage(e));
      });
    return () => {
      cancelled = true;
    };
  }, [searchParams]);

  const selected = useMemo(
    () => permits.find((p) => p.id === selectedId) ?? null,
    [permits, selectedId],
  );

  const clearDeepLink = () => {
    if (searchParams.has(PERMIT_QUERY_CREATE_FOR_WO)) {
      const next = new URLSearchParams(searchParams);
      next.delete(PERMIT_QUERY_CREATE_FOR_WO);
      setSearchParams(next, { replace: true });
    }
  };

  const handleChanged = (updated: WorkPermitRecord) => {
    setPermits((rows) => rows.map((r) => (r.id === updated.id ? updated : r)));
    void load();
  };

  const fmt = (v: string | null) => (v ? formatDateTime(v, i18n.language) : t("detail.none"));

  return (
    <ModulePageShell
      icon={HardHat}
      title={t("page.title")}
      description={t("page.description")}
      bodyClassName="space-y-6 p-4"
      actions={
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => void load()}>
            {t("actions.refresh")}
          </Button>
          {canIssue ? (
            <>
              <Button variant="outline" size="sm" onClick={() => setTypesOpen(true)}>
                {t("actions.manageTypes")}
              </Button>
              <Button
                size="sm"
                onClick={() => {
                  setCreateWo(null);
                  setCreateOpen(true);
                }}
              >
                {t("actions.newPermit")}
              </Button>
            </>
          ) : null}
        </div>
      }
    >
      {error ? (
        <div
          role="alert"
          className="rounded-md border border-status-danger/40 px-3 py-2 text-sm text-status-danger"
        >
          {error}
        </div>
      ) : null}

      {kpi30d ? (
        <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
          <h2 className="mb-2 text-sm font-medium text-text-primary">{t("kpi.title")}</h2>
          <p className="text-sm text-text-muted">
            {t("kpi.summary", {
              activated: kpi30d.activated_count,
              onTime: kpi30d.handed_back_on_time_count,
            })}
            {kpi30d.rate != null
              ? ` · ${t("kpi.rate", { rate: (kpi30d.rate * 100).toFixed(1) })}`
              : ""}
          </p>
        </section>
      ) : null}

      <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-text-primary">{t("open.title")}</h2>
        {openPermits.length === 0 ? (
          <p className="text-sm text-text-muted">{t("open.empty")}</p>
        ) : (
          <ul className="flex flex-wrap gap-2">
            {openPermits.map((row) => (
              <li key={row.id}>
                <Button variant="outline" size="sm" onClick={() => setSelectedId(row.id)}>
                  <span className="font-mono text-xs">{row.code}</span>
                  <span className="ml-2">{t(`status.${row.status}`)}</span>
                </Button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="rounded-lg border border-surface-border bg-surface-1 p-4">
        <h2 className="mb-3 text-sm font-medium text-text-primary">{t("list.title")}</h2>
        <div className="overflow-x-auto">
          <table className="w-full text-left text-sm">
            <thead>
              <tr className="border-b border-surface-border text-text-muted">
                <th className="py-2 pr-4 font-medium">{t("columns.code")}</th>
                <th className="py-2 pr-4 font-medium">{t("columns.status")}</th>
                <th className="py-2 pr-4 font-medium">{t("columns.type")}</th>
                <th className="py-2 pr-4 font-medium">{t("columns.asset")}</th>
                <th className="py-2 pr-4 font-medium">{t("columns.workOrder")}</th>
                <th className="py-2 font-medium">{t("columns.expires")}</th>
              </tr>
            </thead>
            <tbody>
              {permits.map((row) => (
                <tr
                  key={row.id}
                  aria-selected={selectedId === row.id}
                  className={`border-b border-surface-border/60 ${
                    selectedId === row.id ? "bg-surface-2" : ""
                  }`}
                >
                  <td className="py-2 pr-4">
                    <button
                      type="button"
                      className="font-mono text-xs underline-offset-2 hover:underline"
                      aria-label={t("actions.select", { code: row.code })}
                      onClick={() => setSelectedId(row.id)}
                    >
                      {row.code}
                    </button>
                  </td>
                  <td className="py-2 pr-4">
                    <Badge variant="outline">{t(`status.${row.status}`)}</Badge>
                  </td>
                  <td className="py-2 pr-4">{row.permit_type_name ?? t("detail.none")}</td>
                  <td className="py-2 pr-4">{formatAssetLabel(row) ?? t("detail.none")}</td>
                  <td className="py-2 pr-4">{row.linked_work_order_code ?? t("detail.none")}</td>
                  <td className="py-2">{fmt(row.expires_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
          {permits.length === 0 ? (
            <p className="mt-2 text-sm text-text-muted">{t("list.empty")}</p>
          ) : null}
        </div>
      </section>

      {selected ? (
        <PermitDetailPanel permit={selected} onChanged={handleChanged} />
      ) : (
        <p className="text-sm text-text-muted">{t("detail.empty")}</p>
      )}

      <PermitCreateDialog
        open={createOpen}
        onOpenChange={(open) => {
          setCreateOpen(open);
          if (!open) clearDeepLink();
        }}
        types={types}
        workOrder={createWo}
        onCreated={(created) => {
          setPermits((rows) => [created, ...rows]);
          setSelectedId(created.id);
          void load();
        }}
      />
      <PermitTypesDialog
        open={typesOpen}
        onOpenChange={setTypesOpen}
        types={types}
        onSaved={() => void load()}
      />
    </ModulePageShell>
  );
}
