/**
 * "Statuts à revoir (N)": people whose stored employment status is not a usable value of the
 * plant's list (legacy value flagged for review, or a code missing from the list). Each person is
 * listed with the status exactly as stored, so someone can correct it from the person's record.
 * Shown only with the personnel view right (the server's rule); hidden when nobody needs review.
 */
import { AlertTriangle, ChevronDown, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ErrorState } from "@/components/ui/data-state";
import { usePermissions } from "@/hooks/use-permissions";
import { listPersonnelStatusReview } from "@/services/personnel-service";
import type { PersonnelStatusReview } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function PersonnelStatusReviewNotice({
  onOpenPersonnel,
  refreshKey,
  className,
}: {
  onOpenPersonnel: (personnelId: number) => void;
  /** Changes when the directory reloads, so the notice follows corrections. */
  refreshKey?: unknown;
  className?: string;
}) {
  const { t } = useTranslation("personnel");
  const { can } = usePermissions();
  const canView = can(P.PER_VIEW);
  const [rows, setRows] = useState<PersonnelStatusReview[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [open, setOpen] = useState(false);

  const load = useCallback(async () => {
    if (!canView) {
      setRows([]);
      return;
    }
    setError(null);
    try {
      setRows(await listPersonnelStatusReview());
    } catch (err) {
      setError(err);
    }
  }, [canView]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  if (!canView) return null;
  if (error != null) {
    return (
      <ErrorState
        {...(className ? { className } : {})}
        error={error}
        title={t("statusReview.loadError")}
        onRetry={() => void load()}
      />
    );
  }
  if (rows.length === 0) return null;

  return (
    <div
      className={`rounded-md border border-tone-warning-border bg-tone-warning-bg px-3 py-2 text-sm ${className ?? ""}`}
    >
      <button
        type="button"
        className="flex w-full items-center gap-2 text-left font-medium text-tone-warning-fg"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <AlertTriangle className="h-4 w-4 shrink-0" aria-hidden />
        <span className="flex-1">{t("statusReview.title", { count: rows.length })}</span>
        {open ? (
          <ChevronDown className="h-4 w-4" aria-hidden />
        ) : (
          <ChevronRight className="h-4 w-4" aria-hidden />
        )}
      </button>
      {open ? (
        <div className="mt-2 space-y-1">
          <p className="text-xs text-text-secondary">{t("statusReview.hint")}</p>
          <ul className="divide-y divide-surface-border">
            {rows.map((r) => (
              <li key={r.personnel_id} className="flex items-center gap-3 py-1">
                <button
                  type="button"
                  className="min-w-0 flex-1 truncate text-left text-text-primary hover:underline"
                  onClick={() => onOpenPersonnel(r.personnel_id)}
                >
                  <span className="font-mono text-xs text-text-muted">{r.employee_code}</span>{" "}
                  {r.full_name}
                </button>
                <span className="text-xs text-text-secondary">
                  {t("statusReview.stored")}{" "}
                  <code className="font-mono text-text-primary">{r.stored_status}</code>
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
