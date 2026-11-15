/**
 * MyPadlocksPage.tsx — "Mes cadenas" (L4-18).
 *
 * The entry point of a worker holding only `ptw.lock_own`: the permits, at the sites where they hold
 * that right, on which they are a listed worker (issued or active, or still holding their padlock or
 * a forced removal of it they were not told of), each with its workers panel to place or remove
 * their own padlock (the same panel as the permit screen). The server chooses the permits
 * (`list_my_padlock_permits`) and judges every action.
 */

import { Lock } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { PermitWorkersPanel } from "@/components/permits/PermitWorkersPanel";
import { ErrorState } from "@/components/ui/data-state";
import { StatusChip } from "@/components/ui/status-chip";
import { listMyPadlockPermits, type MyPadlockPermit } from "@/services/permit-workers-service";

export function MyPadlocksPage() {
  const { t } = useTranslation("permits");
  const [items, setItems] = useState<MyPadlockPermit[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await listMyPadlockPermits());
      setError(null);
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  return (
    <ModulePageShell
      icon={Lock}
      title={t("myPadlocks.title")}
      count={items?.length ?? null}
      description={t("myPadlocks.hint")}
      actions={<RefreshButton onRefresh={() => void load()} loading={loading} />}
    >
      {error != null ? (
        <ErrorState
          error={error}
          title={t("myPadlocks.loadError")}
          onRetry={() => void load()}
          retrying={loading}
        />
      ) : items == null ? (
        <p className="text-sm text-text-muted" aria-busy="true">
          {t("workers.loading")}
        </p>
      ) : items.length === 0 ? (
        <p className="text-sm text-text-muted" data-testid="my-padlocks-empty">
          {t("myPadlocks.empty")}
        </p>
      ) : (
        <ul className="space-y-4" data-testid="my-padlocks">
          {items.map(({ permit }) => (
            <li key={permit.id} className="space-y-2 rounded-md border border-surface-border p-3">
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium text-text-primary">{permit.code}</span>
                <StatusChip label={t(`status.${permit.status}`, permit.status)} tone="info" />
                {permit.asset_label ? (
                  <span className="text-xs text-text-muted">{permit.asset_label}</span>
                ) : null}
              </div>
              <PermitWorkersPanel
                permit={permit}
                canIssue={false}
                canClose={false}
                canLockOwn
                onChanged={() => void load()}
              />
            </li>
          ))}
        </ul>
      )}
    </ModulePageShell>
  );
}
