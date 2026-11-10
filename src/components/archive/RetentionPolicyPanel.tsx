import { Lock } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  archiveClassHint,
  archiveClassLabel,
  archiveModuleLabel,
  purgeModeLabel,
} from "@/components/archive/archive-labels";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
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
import { StickyActionBar } from "@/components/ui/sticky-action-bar";
import { Switch } from "@/components/ui/switch";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { UnsavedChangesGuard } from "@/components/ui/unsaved-changes-guard";
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

/** What the user changed on one rule, not saved yet. */
interface Draft {
  years: string;
  purge_mode: string;
  allow_restore: boolean;
  allow_purge: boolean;
}

type Changes = Omit<UpdateRetentionInput, "policy_id">;

function draftOf(r: RetentionPolicy): Draft {
  return {
    years: String(r.retention_years),
    purge_mode: r.purge_mode,
    allow_restore: r.allow_restore,
    allow_purge: r.allow_purge,
  };
}

/** The changes of one rule, as the update command takes them; `null` when the years are invalid. */
function changesOf(r: RetentionPolicy, d: Draft): Changes | null {
  if (!/^\d+$/.test(d.years.trim())) return null;
  const years = Number.parseInt(d.years, 10);
  const changes: Changes = {};
  if (years !== r.retention_years) changes.retention_years = years;
  if (d.purge_mode !== r.purge_mode) changes.purge_mode = d.purge_mode;
  if (d.allow_restore !== r.allow_restore) changes.allow_restore = d.allow_restore;
  if (d.allow_purge !== r.allow_purge) changes.allow_purge = d.allow_purge;
  return changes;
}

export function RetentionPolicyPanel() {
  const { t } = useTranslation("archive");
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canEdit = can(P.ADM_SETTINGS);
  const { withStepUp, StepUpDialogElement } = useStepUp();

  const [rows, setRows] = useState<RetentionPolicy[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [drafts, setDrafts] = useState<Record<number, Draft>>({});
  const [saving, setSaving] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const data = await listRetentionPolicies();
      setRows(data);
      setDrafts(Object.fromEntries(data.map((r) => [r.id, draftOf(r)])));
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const nameOf = useCallback(
    (r: RetentionPolicy) =>
      `${archiveModuleLabel(t, r.module_code)} — ${archiveClassLabel(t, r.archive_class)}`,
    [t],
  );

  /** Rules with a change, and the rules whose years cannot be read. */
  const { pending, invalid } = useMemo(() => {
    const pending: { row: RetentionPolicy; changes: Changes }[] = [];
    const invalid: number[] = [];
    for (const row of rows) {
      const draft = drafts[row.id];
      if (!draft) continue;
      const changes = changesOf(row, draft);
      if (changes === null) invalid.push(row.id);
      else if (Object.keys(changes).length > 0) pending.push({ row, changes });
    }
    return { pending, invalid };
  }, [rows, drafts]);
  const dirty = pending.length > 0 || invalid.length > 0;

  /** Changes that lose data or the way back, listed in the confirmation (L7-20). */
  const warnings = useMemo(() => {
    const out: string[] = [];
    for (const { row, changes } of pending) {
      const name = nameOf(row);
      if (changes.retention_years !== undefined && changes.retention_years < row.retention_years) {
        out.push(
          t("retention.warn.shorter", {
            name,
            from: row.retention_years,
            to: changes.retention_years,
          }),
        );
      }
      if (changes.allow_restore === false) out.push(t("retention.warn.noRestore", { name }));
      if (changes.allow_purge === true) out.push(t("retention.warn.purge", { name }));
      if (changes.purge_mode === "scheduled") out.push(t("retention.warn.scheduled", { name }));
    }
    return out;
  }, [pending, nameOf, t]);

  const patch = (id: number, change: Partial<Draft>) =>
    setDrafts((prev) => {
      const current = prev[id];
      if (!current) return prev;
      const next = { ...current, ...change };
      // "Jamais supprimé" cannot allow a deletion.
      if (next.purge_mode === "never") next.allow_purge = false;
      return { ...prev, [id]: next };
    });

  const save = async () => {
    if (!canEdit || pending.length === 0 || invalid.length > 0) return;
    setSaving(true);
    try {
      await withStepUp(async () => {
        for (const { row, changes } of pending) {
          await updateRetentionPolicy({ policy_id: row.id, ...changes });
        }
      });
    } catch {
      // The global toast reports a refusal (e.g. shorter retention for operational history);
      // the reload shows the stored values again.
    } finally {
      setSaving(false);
      setConfirmOpen(false);
      await load();
    }
  };

  const askSave = () => {
    if (warnings.length > 0) setConfirmOpen(true);
    else void save();
  };

  if (loading && rows.length === 0) return <LoadingState label={t("retention.loading")} />;
  if (loadError) {
    return (
      <ErrorState error={loadError} title={t("retention.loadError")} onRetry={() => void load()} />
    );
  }

  const busy = !canEdit || saving;

  return (
    <div className="space-y-3 rounded-lg border border-surface-border p-4">
      {StepUpDialogElement}
      <UnsavedChangesGuard
        when={dirty}
        title={t("retention.unsaved.title")}
        description={t("retention.unsaved.description")}
        confirmLabel={t("retention.unsaved.confirm")}
      />
      <div>
        <h3 className="text-sm font-semibold text-text-primary">{t("retention.title")}</h3>
        <p className="text-xs text-text-muted">{t("retention.subtitle")}</p>
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
              <TableHead className="whitespace-normal">{t("retention.col.module")}</TableHead>
              <TableHead className="whitespace-normal">{t("retention.col.class")}</TableHead>
              <TableHead className="whitespace-normal">{t("retention.col.years")}</TableHead>
              <TableHead className="whitespace-normal">{t("retention.col.purgeMode")}</TableHead>
              <TableHead className="whitespace-normal">{t("retention.col.allowRestore")}</TableHead>
              <TableHead className="whitespace-normal">{t("retention.col.allowPurge")}</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((row) => {
              const draft = drafts[row.id] ?? draftOf(row);
              const purgeLocked = draft.purge_mode === "never";
              const name = nameOf(row);
              const hint = archiveClassHint(t, row.archive_class);
              const yearsInvalid = invalid.includes(row.id);
              return (
                <TableRow key={row.id}>
                  <TableCell>{archiveModuleLabel(t, row.module_code)}</TableCell>
                  <TableCell className="whitespace-normal">
                    <span className="font-medium">{archiveClassLabel(t, row.archive_class)}</span>
                    {hint ? <p className="mt-1 text-[11px] text-text-muted">{hint}</p> : null}
                  </TableCell>
                  <TableCell>
                    <Input
                      type="number"
                      min={0}
                      className="h-9 w-24"
                      aria-label={t("retention.yearsAria", { name })}
                      aria-invalid={yearsInvalid || undefined}
                      disabled={busy}
                      value={draft.years}
                      onChange={(e) => patch(row.id, { years: e.target.value })}
                    />
                    {yearsInvalid ? (
                      <p className="mt-1 text-[11px] text-status-danger">
                        {t("retention.yearsInvalid")}
                      </p>
                    ) : row.archive_class === "operational_history" ? (
                      <p className="mt-1 text-[11px] text-text-muted">
                        {t("retention.noShortening")}
                      </p>
                    ) : null}
                  </TableCell>
                  <TableCell>
                    <Select
                      value={draft.purge_mode}
                      disabled={busy}
                      onValueChange={(value) => patch(row.id, { purge_mode: value })}
                    >
                      <SelectTrigger className="h-9 w-48" aria-label={t("retention.col.purgeMode")}>
                        <SelectValue>{purgeModeLabel(t, draft.purge_mode)}</SelectValue>
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
                      checked={draft.allow_restore}
                      disabled={busy}
                      aria-label={t("retention.allowRestoreAria", { name })}
                      onCheckedChange={(checked) => patch(row.id, { allow_restore: checked })}
                    />
                  </TableCell>
                  <TableCell>
                    <Switch
                      checked={draft.allow_purge}
                      disabled={busy || purgeLocked}
                      aria-label={t("retention.allowPurgeAria", { name })}
                      onCheckedChange={(checked) => patch(row.id, { allow_purge: checked })}
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

      {canEdit && rows.length > 0 ? (
        <StickyActionBar
          hint={
            dirty ? t("retention.pending", { count: pending.length + invalid.length }) : undefined
          }
        >
          <div className="flex justify-end gap-2">
            <Button
              variant="outline"
              disabled={!dirty || saving}
              onClick={() => setDrafts(Object.fromEntries(rows.map((r) => [r.id, draftOf(r)])))}
            >
              {t("retention.cancel")}
            </Button>
            <Button
              disabled={pending.length === 0 || invalid.length > 0 || saving}
              onClick={askSave}
            >
              {t("retention.save")}
            </Button>
          </div>
        </StickyActionBar>
      ) : null}

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t("retention.warn.title")}
        description={t("retention.warn.description")}
        confirmLabel={t("retention.warn.confirm")}
        destructive
        confirmDisabled={saving}
        onConfirm={save}
      >
        <ul className="list-disc space-y-1 pl-5 text-sm">
          {warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      </ConfirmDialog>
    </div>
  );
}
