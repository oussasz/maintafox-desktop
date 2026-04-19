import { Activity, FlaskConical, Gauge, Layers, LayoutDashboard, ShieldCheck } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { NavLink, Outlet } from "react-router-dom";

import { mfChip, mfInput, mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { listAssets } from "@/services/asset-service";
import { iso14224FailureDatasetCompleteness } from "@/services/reliability-service";
import { useLocaleStore } from "@/stores/locale-store";
import type { Asset } from "@shared/ipc-types";
import type { Iso14224DatasetCompleteness } from "@shared/ipc-types";

import { RamsEquipmentProvider, useRamsEquipment } from "./rams-equipment-context";

const NAV = [
  { to: "dashboard", key: "nav.dashboard", icon: LayoutDashboard },
  { to: "foundation", key: "nav.foundation", icon: Layers },
  { to: "lab", key: "nav.lab", icon: FlaskConical },
  { to: "advanced", key: "nav.advanced", icon: Gauge },
  { to: "governance", key: "nav.governance", icon: ShieldCheck },
] as const;

function ReliabilityModuleLayoutBody() {
  const { t } = useTranslation("reliability");
  const rtl = useLocaleStore((s) => s.direction === "rtl");
  const {
    selectedEquipmentId,
    selectedEquipmentIds,
    setSelectedEquipmentId,
    setSelectedEquipmentIds,
    toggleSelectedEquipmentId,
  } = useRamsEquipment();
  const [assets, setAssets] = useState<Asset[]>([]);
  const [assetsLoading, setAssetsLoading] = useState(true);
  const [assetQuery, setAssetQuery] = useState("");
  const [iso, setIso] = useState<Iso14224DatasetCompleteness | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      setAssetsLoading(true);
      try {
        const rows = await listAssets(null, null, null, 2000);
        if (!cancelled) {
          setAssets(rows.filter((a) => a.deleted_at == null));
        }
      } catch {
        if (!cancelled) {
          setAssets([]);
        }
      } finally {
        if (!cancelled) {
          setAssetsLoading(false);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!assets.length || selectedEquipmentIds.length === 0) {
      return;
    }
    const valid = selectedEquipmentIds.filter((id) => assets.some((a) => a.id === id));
    if (valid.length !== selectedEquipmentIds.length) {
      setSelectedEquipmentIds(valid);
    }
  }, [assets, selectedEquipmentIds, setSelectedEquipmentIds]);

  useEffect(() => {
    if (selectedEquipmentId == null) {
      setIso(null);
      return;
    }
    void iso14224FailureDatasetCompleteness(selectedEquipmentId)
      .then(setIso)
      .catch(() => setIso(null));
  }, [selectedEquipmentId]);

  const filteredAssets = useMemo(() => {
    const q = assetQuery.trim().toLowerCase();
    if (!q) {
      return assets;
    }
    return assets.filter(
      (a) =>
        a.asset_code.toLowerCase().includes(q) ||
        a.asset_name.toLowerCase().includes(q) ||
        String(a.id).includes(q),
    );
  }, [assets, assetQuery]);

  const selectedAsset = useMemo(
    () =>
      selectedEquipmentId == null
        ? null
        : (assets.find((a) => a.id === selectedEquipmentId) ?? null),
    [assets, selectedEquipmentId],
  );

  const pct = iso?.completeness_percent;
  const isoChipClass =
    pct == null
      ? mfChip.neutral
      : pct >= 85
        ? cn(mfChip.neutralStrong, "border-status-success/35 text-text-success")
        : pct >= 60
          ? cn(mfChip.neutralStrong, "border-status-warning/35 text-text-warning")
          : cn(mfChip.neutralStrong, "border-status-danger/35 text-text-danger");

  const isoBreakdownTitle = iso
    ? [
        t("health.isoTitle"),
        "",
        t("health.isoDimEquipment", { pct: iso.dim_equipment_id_pct.toFixed(1) }),
        t("health.isoDimInterval", { pct: iso.dim_failure_interval_pct.toFixed(1) }),
        t("health.isoDimMode", { pct: iso.dim_failure_mode_pct.toFixed(1) }),
        t("health.isoDimClosure", { pct: iso.dim_corrective_closure_pct.toFixed(1) }),
      ].join("\n")
    : t("health.isoTitle");

  return (
    <div className={cn(mfLayout.moduleRoot, "bg-surface-0")}>
      <header className={mfLayout.moduleHeader}>
        <div className="flex min-w-0 items-center gap-2">
          <Activity className={mfLayout.moduleHeaderIcon} aria-hidden />
          <div className="min-w-0">
            <h1 className={mfLayout.moduleTitle}>{t("module.title")}</h1>
            <p className="truncate text-xs text-text-muted">{t("module.subtitle")}</p>
          </div>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2 text-xs">
          <div className="max-w-[min(100%,14rem)] truncate text-end text-text-secondary">
            {selectedAsset && selectedEquipmentIds.length <= 1 ? (
              <span
                className="font-mono text-[11px] text-text-primary"
                title={selectedAsset.asset_name}
              >
                {selectedAsset.asset_code}
                <span className="ms-1 text-text-muted">{selectedAsset.asset_name}</span>
              </span>
            ) : selectedEquipmentIds.length > 1 ? (
              <span className="font-mono text-[11px] text-text-primary">
                {t("module.multiSelected", { count: selectedEquipmentIds.length })}
              </span>
            ) : (
              <span className="text-text-muted">{t("health.equipNone")}</span>
            )}
          </div>
          <div
            className={cn("rounded-full px-2.5 py-1 text-xs font-medium", isoChipClass)}
            title={isoBreakdownTitle}
          >
            {t("health.isoBadge")}
            {iso && iso.event_count > 0 ? (
              <span className="ml-1.5 font-mono tabular-nums">
                {iso.completeness_percent.toFixed(1)}%
              </span>
            ) : (
              <span className="ml-1.5 text-text-muted">{t("health.isoEmpty")}</span>
            )}
          </div>
          <NavLink
            to="/reliability/foundation#ram-data-quality"
            className="rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-xs text-text-secondary hover:bg-surface-3"
          >
            {t("health.openDq")}
          </NavLink>
        </div>
      </header>

      <div className={cn("flex min-h-0 flex-1", rtl ? "flex-row-reverse" : "flex-row")}>
        <aside className="flex w-60 shrink-0 flex-col border-e border-surface-border bg-surface-1 py-3">
          <nav className="flex shrink-0 flex-col gap-0.5 px-2">
            {NAV.map(({ to, key, icon: Icon }) => (
              <NavLink
                key={to}
                to={`/reliability/${to}`}
                className={({ isActive }) =>
                  cn(
                    "flex items-center gap-2 rounded-lg px-3 py-2 text-sm transition-colors",
                    isActive
                      ? "border border-primary/30 bg-surface-2 text-text-primary shadow-sm"
                      : "border border-transparent text-text-muted hover:border-surface-border hover:bg-surface-2/80 hover:text-text-primary",
                  )
                }
              >
                <Icon className="h-4 w-4 shrink-0 opacity-90" aria-hidden />
                <span className="truncate">{t(key)}</span>
              </NavLink>
            ))}
          </nav>

          <div className="mt-3 border-t border-surface-border px-2 pt-3">
            <p className="mb-1.5 px-1 text-[10px] font-medium uppercase tracking-wide text-text-muted">
              {t("module.sidebarEquip")}
            </p>
            <input
              type="search"
              className={cn(
                "mb-2 w-full rounded-md border border-surface-border bg-surface-2 px-2 py-1 text-[11px] text-text-primary placeholder:text-text-muted",
                mfInput.filterSelect,
              )}
              placeholder={t("module.searchAssets")}
              value={assetQuery}
              onChange={(e) => setAssetQuery(e.target.value)}
              aria-label={t("module.searchAssets")}
            />
            <div className="max-h-[min(50vh,22rem)] overflow-y-auto rounded-md border border-surface-border bg-surface-0">
              {assetsLoading ? (
                <p className="p-2 text-[11px] text-text-muted">{t("module.assetsLoading")}</p>
              ) : filteredAssets.length === 0 ? (
                <p className="p-2 text-[11px] text-text-muted">{t("module.assetsEmpty")}</p>
              ) : (
                <ul className="divide-y divide-surface-border">
                  {filteredAssets.map((a) => {
                    const active = selectedEquipmentIds.includes(a.id);
                    const primary = selectedEquipmentId === a.id;
                    return (
                      <li key={a.id}>
                        <button
                          type="button"
                          className={cn(
                            "w-full border-l-4 px-2 py-1.5 text-start text-[11px] transition-colors",
                            active
                              ? "border-l-blue-600 bg-blue-600/10 text-blue-700 dark:text-blue-300"
                              : "text-text-secondary hover:bg-surface-2",
                          )}
                          onClick={(evt) => {
                            if (evt.metaKey || evt.ctrlKey) {
                              toggleSelectedEquipmentId(a.id);
                              return;
                            }
                            setSelectedEquipmentId(a.id);
                          }}
                        >
                          <span className="flex items-center gap-1.5">
                            <input
                              type="checkbox"
                              checked={active}
                              onChange={() => toggleSelectedEquipmentId(a.id)}
                              onClick={(evt) => evt.stopPropagation()}
                              aria-label={t("module.selectEquipment")}
                            />
                            <span
                              className={cn(
                                "block font-mono",
                                active ? "text-blue-700 dark:text-blue-300" : "text-text-primary",
                              )}
                            >
                              {a.asset_code}
                            </span>
                            {primary ? (
                              <span className="rounded border border-blue-600/40 bg-blue-600/10 px-1 py-0.5 text-[9px] uppercase tracking-wide text-blue-700 dark:text-blue-300">
                                {t("module.primary")}
                              </span>
                            ) : null}
                          </span>
                          <span className="block truncate text-text-muted">{a.asset_name}</span>
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          </div>
        </aside>

        <main className="min-h-0 min-w-0 flex-1 overflow-auto bg-surface-0">
          {selectedEquipmentId == null ? (
            <div className="flex min-h-[min(70vh,32rem)] flex-col items-center justify-center gap-2 px-6 text-center">
              <p className="max-w-md text-sm text-text-muted">
                {t("module.selectEquipmentPrompt")}
              </p>
            </div>
          ) : (
            <Outlet />
          )}
        </main>
      </div>
    </div>
  );
}

/** RAMS shell: shared equipment selection (sidebar) and ISO health chip. */
export function ReliabilityModuleLayout() {
  return (
    <RamsEquipmentProvider>
      <ReliabilityModuleLayoutBody />
    </RamsEquipmentProvider>
  );
}
