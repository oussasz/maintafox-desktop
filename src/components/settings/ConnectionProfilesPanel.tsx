import { Link2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { useFormatters } from "@/hooks/use-formatters";
import {
  listConnectionProfiles,
  type ConnectionProfile,
} from "@/services/connection-profiles-service";

interface ConnectionProfilesPanelProps {
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
}

/**
 * Connection profiles of this site. A failed load is an error with retry in place of the list —
 * it used to read "no profiles" under a toast. `onToast` is kept for the page's API.
 */
export function ConnectionProfilesPanel(_props: ConnectionProfilesPanelProps) {
  const { t } = useTranslation("settings");
  const { formatDateTime } = useFormatters();
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [attempt, setAttempt] = useState(0);
  const retry = useCallback(() => setAttempt((n) => n + 1), []);

  const formatProfileStatus = (status: string) => {
    const key = `integrations.status.${status}` as "integrations.status.active";
    const translated = t(key);
    return translated === key ? status : translated;
  };

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setLoadError(null);
    listConnectionProfiles()
      .then((rows) => {
        if (!cancelled) setProfiles(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setLoadError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  return (
    <div className="space-y-3 rounded-lg border border-border p-4">
      <div className="flex items-start gap-2">
        <Link2 className="mt-0.5 h-4 w-4 text-text-muted" aria-hidden />
        <div>
          <h3 className="text-base font-semibold">{t("integrations.profilesTitle")}</h3>
          <p className="text-sm text-text-muted">{t("integrations.profilesDescription")}</p>
        </div>
      </div>

      {loading ? (
        <LoadingState label={t("page.loading")} className="py-4" />
      ) : loadError != null ? (
        <ErrorState error={loadError} title={t("integrations.profilesLoadError")} onRetry={retry} />
      ) : profiles.length === 0 ? (
        <p className="text-sm text-text-muted">{t("integrations.profilesEmpty")}</p>
      ) : (
        <div className="overflow-auto rounded-md border border-border">
          <table className="w-full text-sm">
            <thead className="bg-muted/50 text-left text-text-muted">
              <tr>
                <th className="px-3 py-2">{t("integrations.colName")}</th>
                <th className="px-3 py-2">{t("integrations.colType")}</th>
                <th className="px-3 py-2">{t("integrations.colStatus")}</th>
                <th className="px-3 py-2">{t("integrations.colLastTest")}</th>
              </tr>
            </thead>
            <tbody>
              {profiles.map((p) => (
                <tr key={p.id} className="border-t border-border">
                  <td className="px-3 py-2 font-medium">{p.profile_name}</td>
                  <td className="px-3 py-2">{p.integration_type}</td>
                  <td className="px-3 py-2">{formatProfileStatus(p.status)}</td>
                  <td className="px-3 py-2 text-text-muted">
                    {p.last_tested_at ? formatDateTime(p.last_tested_at) : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
