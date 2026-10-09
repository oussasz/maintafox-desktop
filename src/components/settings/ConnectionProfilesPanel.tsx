import { Link2, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

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

export function ConnectionProfilesPanel({ onToast }: ConnectionProfilesPanelProps) {
  const { t } = useTranslation("settings");
  const { formatDateTime } = useFormatters();
  const [profiles, setProfiles] = useState<ConnectionProfile[]>([]);
  const [loading, setLoading] = useState(true);

  const formatProfileStatus = (status: string) => {
    const key = `integrations.status.${status}` as "integrations.status.active";
    const translated = t(key);
    return translated === key ? status : translated;
  };

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    listConnectionProfiles()
      .then((rows) => {
        if (!cancelled) setProfiles(rows);
      })
      .catch((err) => {
        if (!cancelled) {
          onToast({
            title: t("integrations.profilesLoadError"),
            description: String(err),
            variant: "destructive",
          });
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [onToast, t]);

  return (
    <div className="space-y-3 rounded-lg border border-border p-4">
      <div className="flex items-start gap-2">
        <Link2 className="mt-0.5 h-4 w-4 text-text-muted" />
        <div>
          <h3 className="text-base font-semibold">{t("integrations.profilesTitle")}</h3>
          <p className="text-sm text-text-muted">{t("integrations.profilesDescription")}</p>
        </div>
      </div>

      {loading ? (
        <div className="flex items-center gap-2 text-sm text-text-muted">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("page.loading")}
        </div>
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
