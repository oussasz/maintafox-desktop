import { AlertTriangle, MonitorSmartphone, Shield, Users } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { getAdminStats } from "@/services/rbac-service";
import type { AdminStatsPayload } from "@shared/ipc-types";

const REFRESH_INTERVAL_MS = 60_000;

interface MetricCard {
  labelKey: string;
  icon: React.ElementType;
  color: string;
  getValue: (s: AdminStatsPayload) => number;
  getBadge?: (s: AdminStatsPayload) => string | null;
  alertWhen?: (s: AdminStatsPayload) => boolean;
}

const CARDS: MetricCard[] = [
  {
    labelKey: "admin:metrics.activeUsers",
    icon: Users,
    color: "text-blue-600 bg-blue-100",
    getValue: (s) => s.active_users,
  },
  {
    labelKey: "admin:metrics.roles",
    icon: Shield,
    color: "text-indigo-600 bg-indigo-100",
    getValue: (s) => s.total_roles,
    getBadge: (s) => `${s.system_roles} sys / ${s.custom_roles} custom`,
  },
  {
    labelKey: "admin:metrics.activeSessions",
    icon: MonitorSmartphone,
    color: "text-emerald-600 bg-emerald-100",
    getValue: (s) => s.active_sessions,
  },
  {
    labelKey: "admin:metrics.unassigned",
    icon: AlertTriangle,
    color: "text-amber-600 bg-amber-100",
    getValue: (s) => s.unassigned_users,
    alertWhen: (s) => s.unassigned_users > 0,
  },
];

export function AdminMetricCards() {
  const { t } = useTranslation("admin");
  const [stats, setStats] = useState<AdminStatsPayload | null>(null);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const load = useCallback(async () => {
    try {
      const data = await getAdminStats();
      setStats(data);
    } catch {
      // Keep previous stats on error
    }
  }, []);

  useEffect(() => {
    void load();
    timerRef.current = setInterval(() => void load(), REFRESH_INTERVAL_MS);
    return () => {
      if (timerRef.current) clearInterval(timerRef.current);
    };
  }, [load]);

  return (
    <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {CARDS.map((card) => {
        const IconComp = card.icon;
        const value = stats ? card.getValue(stats) : null;
        const badge = stats && card.getBadge ? card.getBadge(stats) : null;
        const isAlert = stats && card.alertWhen ? card.alertWhen(stats) : false;

        return (
          <div
            key={card.labelKey}
            className={`flex items-center gap-4 rounded-lg border p-4 transition-colors
              ${isAlert ? "border-status-danger/30 bg-status-danger/5" : "border-surface-border bg-surface-1"}`}
          >
            <div
              className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${card.color}`}
            >
              <IconComp className="h-5 w-5" />
            </div>
            <div className="min-w-0">
              <p className="text-sm text-text-secondary">{t(card.labelKey)}</p>
              <p
                className={`text-xl font-semibold ${isAlert ? "text-status-danger" : "text-text-primary"}`}
              >
                {value !== null ? value : "—"}
              </p>
              {badge && <p className="text-xs text-text-muted">{badge}</p>}
            </div>
          </div>
        );
      })}
    </div>
  );
}
