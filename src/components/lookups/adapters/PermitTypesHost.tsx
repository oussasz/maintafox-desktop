/**
 * Données de référence host of the work-permit types (catalog `PERMIT.TYPES`, live table).
 * Reading needs the plant-wide permit view right and editing the plant-wide permit issuer right
 * (rights held at no node) — the same rights as the `list_permit_types` / `upsert_permit_type`
 * commands; the reference rights do not widen them.
 * Also opened from the Permits page for permit issuers without access to Données de référence
 * (one editor, two entry points).
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { usePermitPlantWideAccess } from "@/components/permits/PermitSiteScope";
import { PermitTypesEditor } from "@/components/permits/PermitTypesEditor";
import { PERMIT_REFUSAL_NAMESPACES } from "@/components/permits/permit-rules";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { i18n } from "@/i18n";
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
  // Permit types are plant-wide (both commands check the right at no node): the rights at no
  // node count, not a right held on one site.
  const { can, loading: rightsLoading } = usePermitPlantWideAccess();
  const canView = can(P.PTW_VIEW);
  const canEdit = can(P.PTW_ISSUE);

  useEffect(() => {
    void i18n.loadNamespaces([...PERMIT_REFUSAL_NAMESPACES]);
  }, []);

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

      {rightsLoading ? (
        <LoadingState />
      ) : !canView ? (
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
