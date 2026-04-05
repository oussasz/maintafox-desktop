import { useTranslation } from "react-i18next";

import { useAppStore } from "@/store/app-store";

export function DashboardPage() {
  const { t } = useTranslation("shell");
  const appVersion = useAppStore((s) => s.appVersion);
  const isOnline = useAppStore((s) => s.isOnline);

  return (
    <div className="p-6 space-y-4">
      <h1 className="text-2xl font-semibold text-text-primary">Maintafox</h1>
      <p className="text-text-secondary">
        {t("startup.loading")}... Phase 1 — infrastructure en place.
      </p>
      <div className="grid grid-cols-2 gap-3 max-w-sm mt-4">
        <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
          <p className="text-xs text-text-muted">Version</p>
          <p className="text-lg font-mono text-text-primary">{appVersion || "—"}</p>
        </div>
        <div className="rounded-lg bg-surface-2 border border-surface-border p-4">
          <p className="text-xs text-text-muted">Connexion</p>
          <p
            className={`text-lg font-semibold ${isOnline ? "text-status-success" : "text-status-warning"}`}
          >
            {isOnline ? "En ligne" : "Hors ligne"}
          </p>
        </div>
      </div>
    </div>
  );
}
