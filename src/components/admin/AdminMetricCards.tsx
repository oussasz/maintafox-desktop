/**
 * Administration headline figures on the shared KpiTile: grey by default, a tone only for an
 * exception (people without a role). Each tile opens the tab behind its figure. A failed load is
 * said (with retry) instead of keeping stale numbers silently.
 */
import { AlertTriangle, MonitorSmartphone, Shield, Users } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { KpiTile } from "@/components/metrics/KpiTile";
import { ErrorState } from "@/components/ui/data-state";
import { useFormatters } from "@/hooks/use-formatters";
import { getAdminStats } from "@/services/rbac-service";
import type { AdminStatsPayload } from "@shared/ipc-types";

const REFRESH_INTERVAL_MS = 60_000;

export type AdminMetricTarget = "users" | "roles" | "sessions";

interface AdminMetricCardsProps {
  /** Opens the admin tab behind a figure; omitted when the user cannot open it. */
  onOpen?: (target: AdminMetricTarget) => void;
  /** Tabs the user may open (a tile without a reachable tab is not a button). */
  canOpen?: (target: AdminMetricTarget) => boolean;
}

export function AdminMetricCards({ onOpen, canOpen }: AdminMetricCardsProps = {}) {
  const { t } = useTranslation("admin");
  const { formatNumber } = useFormatters();
  const [stats, setStats] = useState<AdminStatsPayload | null>(null);
  const [error, setError] = useState<unknown>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const load = useCallback(async () => {
    try {
      setStats(await getAdminStats());
      setError(null);
    } catch (err) {
      setError(err);
    }
  }, []);

  useEffect(() => {
    void load();
    timerRef.current = setInterval(() => void load(), REFRESH_INTERVAL_MS);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [load]);

  const opener = (target: AdminMetricTarget) =>
    onOpen && (canOpen?.(target) ?? true) ? { onOpen: () => onOpen(target) } : {};
  const value = (n: number | undefined) => (n == null ? "—" : formatNumber(n));

  if (error != null && stats == null) {
    return (
      <ErrorState
        className="py-4"
        error={error}
        title={t("metrics.loadError")}
        onRetry={() => void load()}
      />
    );
  }

  return (
    <div className="space-y-2">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
        <KpiTile
          label={t("metrics.activeUsers")}
          value={value(stats?.active_users)}
          icon={Users}
          {...opener("users")}
        />
        <KpiTile
          label={t("metrics.roles")}
          value={value(stats?.total_roles)}
          icon={Shield}
          {...(stats
            ? {
                hint: t("metrics.rolesSplit", {
                  system: formatNumber(stats.system_roles),
                  custom: formatNumber(stats.custom_roles),
                }),
              }
            : {})}
          {...opener("roles")}
        />
        <KpiTile
          label={t("metrics.activeSessions")}
          value={value(stats?.active_sessions)}
          icon={MonitorSmartphone}
          {...opener("sessions")}
        />
        <KpiTile
          label={t("metrics.unassigned")}
          value={value(stats?.unassigned_users)}
          icon={AlertTriangle}
          hint={t("metrics.unassignedHint")}
          {...(stats && stats.unassigned_users > 0 ? { tone: "attention" as const } : {})}
          {...opener("users")}
        />
      </div>
      {error != null ? (
        // Figures above are from the previous successful load.
        <p role="status" className="text-xs text-tone-warning-fg">
          {t("metrics.stale")}
        </p>
      ) : null}
    </div>
  );
}
