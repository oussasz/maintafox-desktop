/**
 * Crews that belong to no planning site (no org node at or above their primary node whose type
 * can own work): they appear on no weekly schedule, so their work is never approved, frozen or
 * published. Shown on the planning page with the two places that fix it — the crew's primary
 * node (Personnel › Teams) and the organisation's node types (Organisation).
 */

import { AlertTriangle } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { errorText } from "@/components/planning/weekly-schedule-model";
import { listCrewsWithoutPlanningSite } from "@/services/planning-service";
import type { WeeklyCrew } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function CrewsWithoutSiteBanner() {
  const { t } = useTranslation("planning");
  const [crews, setCrews] = useState<WeeklyCrew[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    listCrewsWithoutPlanningSite()
      .then((rows) => {
        if (!cancelled) setCrews(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(errorText(err));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return (
      <div
        role="alert"
        className="border-b border-destructive/20 bg-destructive/5 px-6 py-2 text-sm text-destructive"
      >
        {t("crewsWithoutSite.loadFailed", { error })}
      </div>
    );
  }
  if (crews.length === 0) return null;

  return (
    <div
      role="status"
      className="flex flex-wrap items-start gap-2 border-b border-amber-200 bg-amber-50 px-6 py-2 text-sm text-amber-900"
    >
      <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0" aria-hidden />
      <div className="min-w-0 flex-1">
        <p className="font-medium">{t("crewsWithoutSite.title")}</p>
        <p>{crews.map((c) => `${c.team_code} — ${c.team_name}`).join(", ")}</p>
        <p className="text-xs">{t("crewsWithoutSite.hint")}</p>
      </div>
      <div className="flex flex-wrap gap-3 text-xs font-medium">
        <PermissionGate permission={P.PER_VIEW}>
          <Link className="underline" to="/personnel?tab=teams">
            {t("crewsWithoutSite.fixTeams")}
          </Link>
        </PermissionGate>
        <PermissionGate permission={P.ORG_VIEW}>
          <Link className="underline" to="/org">
            {t("crewsWithoutSite.fixOrg")}
          </Link>
        </PermissionGate>
      </div>
    </div>
  );
}
