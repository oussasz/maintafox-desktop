import { Lock } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  archiveClassLabel,
  archiveModuleLabel,
  purgeModeLabel,
} from "@/components/archive/archive-labels";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import {
  type RetentionPolicy,
  type UpdateRetentionInput,
  listRetentionPolicies,
  updateRetentionPolicy,
} from "@/services/archive-service";
import { P } from "@shared/rbac/permissions.generated";

/** Purge modes accepted by `update_retention_policy`. */
const PURGE_MODES = ["manual_approval", "scheduled", "never"] as const;

export function RetentionPolicyPanel() {
  const { t } = useTranslation("archive");
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canEdit = can(P.ADM_SETTINGS);
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [rows, setRows] = useState<RetentionPolicy[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [savingId, setSavingId] = useState<number | null>(null);
  const [yearDrafts, setYearDrafts] = useState<Record<number, string>>({});

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const data = await listRetentionPolicies();
      setRows(data);
      setYearDrafts(Object.fromEntries(data.map((r) => [r.id, String(r.retention_years)])));
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const patchPolicy = useCallback(
    async (policy: RetentionPolicy, changes: Omit<UpdateRetentionInput, "policy_id">) => {
      if (!canEdit) return;
      setSavingId(policy.id);
      setRows((prev) => prev.map((r) => (r.id === policy.id ? { ...r, ...changes } : r)));
      try {
        await withStepUp(() => updateRetentionPolicy({ policy_id: policy.id, ...changes }));
      } catch {
        // The global toast reports a refusal (e.g. shorter retention for operational history);
        // reload to show the stored values again.
      } finally {
        setSavingId(null);
        await load();
      }
    },
    [canEdit, load, withStepUp],
  );

  const commitYears = (policy: RetentionPolicy) => {
    const draft = yearDrafts[policy.id] ?? String(policy.retention_years);
    const next = Number.parseInt(draft, 10);
    if (!Number.isInteger(next) || next < 0) {
      setYearDrafts((prev) => ({ ...prev, [policy.id]: String(policy.retention_years) }));
      return;
    }
    if (next !== policy.retention_years) void patchPolicy(policy, { retention_years: next });
  };

  if (loading && rows.length === 0) return <LoadingState label={t("retention.loading")} />;
  if (loadError) {
    return (
      <ErrorState error={loadError} title={t("retention.loadError")} onRetry={() => void load()} />
    );
  }

  return (
    <div className="space-y-3 rounded-lg border border-surface-border p-4">
      {StepUpDialogElement}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div>
          <h3 className="text-sm font-semibold text-text-primary">{t("retention.title")}</h3>
          <p className="text-xs text-text-muted">{t("retention.subtitle")}</p>
        </div>
      </div>
      {!permissionsLoading && !canEdit ? (
        <div className="flex items-center gap-2 text-xs text-text-muted">
          <Lock className="h-3.5 w-3.5" aria-hidden />
          {t("retention.readOnly")}
        </div>
      ) : null}

      {rows.length === 0 ? (
        <EmptyState title={t("retention.empty")} />
      ) : (
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>{t("retention.col.module")}</TableHead>
              <TableHead>{t("retention.col.class")}</TableHead>
              <TableHead>{t("retention.col.years")}</TableHead>
              <TableHead>{t("retention.col.purgeMode")}</TableHead>
              <TableHead>{t("retention.col.allowRestore")}</TableHead>
              <TableHead>{t("retention.col.allowPurge")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => {
              const busy = !canEdit || savingId === row.id;
              const purgeLocked = row.purge_mode === "never";
              const name = `${archiveModuleLabel(t, row.module_code)} — ${archiveClassLabel(t, row.archive_class)}`;
              return (
                <TableRow key={row.id}>
                  <TableCell>{archiveModuleLabel(t, row.module_code)}</TableCell>
                  <TableCell>{archiveClassLabel(t, row.archive_class)}</TableCell>
                  <TableCell>
                    <Input
                      type="number"
                      min={0}
                      className="h-9 w-24"
                      aria-label={t("retention.yearsAria", { name })}
                      disabled={busy}
                      value={yearDrafts[row.id] ?? String(row.retention_years)}
                      onChange={(e) =>
                        setYearDrafts((prev) => ({ ...prev, [row.id]: e.target.value }))
                      }
                      onBlur={() => commitYears(row)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          commitYears(row);
                        }
                      }}
                    />
                    {row.archive_class === "operational_history" ? (
                      <p className="mt-1 text-[11px] text-text-muted">
                        {t("retention.noShortening")}
                      </p>
                    ) : null}
                  </TableCell>
                  <TableCell>
                    <Select
                      value={row.purge_mode}
                      disabled={busy}
                      onValueChange={(value) =>
                        void patchPolicy(row, {
                          purge_mode: value,
                          ...(value === "never" && row.allow_purge ? { allow_purge: false } : {}),
                        })
                      }
                    >
                      <SelectTrigger className="h-9 w-48" aria-label={t("retention.col.purgeMode")}>
                        <SelectValue>{purgeModeLabel(t, row.purge_mode)}</SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {PURGE_MODES.map((m) => (
                          <SelectItem key={m} value={m}>
                            {purgeModeLabel(t, m)}
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={row.allow_restore}
                      disabled={busy}
                      aria-label={t("retention.allowRestoreAria", { name })}
                      onCheckedChange={(checked) =>
                        void patchPolicy(row, { allow_restore: checked })
                      }
                    />
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={row.allow_purge}
                      disabled={busy || purgeLocked}
                      aria-label={t("retention.allowPurgeAria", { name })}
                      onCheckedChange={(checked) => void patchPolicy(row, { allow_purge: checked })}
                    />
                    {purgeLocked ? (
                      <p className="mt-1 text-[11px] text-text-muted">
                        {t("retention.purgeLocked")}
                      </p>
                    ) : null}
                  </TableCell>
                </TableRow>
              );
            })}
          </TableBody>
        </Table>
      )}
    </div>
  );
}
