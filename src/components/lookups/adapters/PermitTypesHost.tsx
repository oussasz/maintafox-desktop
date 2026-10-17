/**
 * Données de référence host of the work-permit types (catalog `PERMIT.TYPES`, live table).
 * Reading needs the permit view right and editing the permit issuer right — the same rights as
 * the `list_permit_types` / `upsert_permit_type` commands; the reference rights do not widen them.
 * Also opened from the Permits page for permit issuers without access to Données de référence
 * (one editor, two entry points).
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermitTypesEditor } from "@/components/permits/PermitTypesEditor";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { usePermissions } from "@/hooks/use-permissions";
import { type PermitTypeRecord, listPermitTypes } from "@/services/permit-service";
import { P } from "@shared/rbac/permissions.generated";

interface PermitTypesHostProps {
  /** Inside a dialog that already shows the title. */
  embedded?: boolean;
  /** Called after a type is saved (the Permits page reloads its type filter). */
  onSaved?: () => void;
}

export function PermitTypesHost({ embedded = false, onSaved }: PermitTypesHostProps = {}) {
  const { t } = useTranslation("reference");
  const { t: tp } = useTranslation("permits");
  const { can } = usePermissions();
  const canView = can(P.PTW_VIEW);
  const canEdit = can(P.PTW_ISSUE);

  const [types, setTypes] = useState<PermitTypeRecord[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      setTypes(await listPermitTypes());
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (canView) void load();
  }, [canView, load]);

  return (
    <section
      className={
        embedded ? "flex flex-col gap-3" : "flex h-full min-h-0 flex-col gap-3 overflow-y-auto p-4"
      }
    >
      {embedded ? null : (
        <div className="flex flex-wrap items-center gap-2">
          <h2 className="text-sm font-semibold text-text-primary">{tp("types.title")}</h2>
          <StatusChip
            label={t("synthetic.liveTable")}
            tone="info"
            title={t("synthetic.liveTableHint")}
          />
        </div>
      )}
      <p className="text-xs text-text-muted">{tp("types.energyIsolationHint")}</p>

      {!canView ? (
        <EmptyState title={tp("types.noViewRight")} />
      ) : loadError != null ? (
        <ErrorState error={loadError} onRetry={() => void load()} retrying={loading} />
      ) : types == null ? (
        <LoadingState />
      ) : (
        <PermitTypesEditor
          types={types}
          canEdit={canEdit}
          onSaved={() => {
            void load();
            onSaved?.();
          }}
        />
      )}
    </section>
  );
}
