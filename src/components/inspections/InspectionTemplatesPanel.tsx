/**
 * Checklists and routes: list, create and publish a new version (versions are never edited in
 * place, PRD §6.25). A checklist's ordered, equipment-bound checks are the route walked in the
 * field; checks of another checklist can be inserted for a given equipment (one stop per asset).
 * Checks pick their equipment with the asset picker; no ids are typed. Every row problem is
 * shown on the row before anything is sent.
 */
import type { ColumnDef } from "@tanstack/react-table";
import { ArrowDown, ArrowUp, Copy, Loader2, Pencil, Plus, Trash2 } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import { DataTable } from "@/components/data/DataTable";
import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  FormField,
  Input,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@/components/ui";
import { Checkbox } from "@/components/ui/checkbox";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { usePermissions } from "@/hooks/use-permissions";
import { getAssetByIdSilent } from "@/services/asset-service";
import {
  createInspectionTemplate,
  listInspectionCheckpoints,
  listInspectionTemplateVersions,
  listInspectionTemplates,
  publishInspectionTemplateVersion,
} from "@/services/inspection-service";
import type {
  InspectionCheckpoint,
  InspectionCheckpointDraft,
  InspectionTemplate,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

import {
  CHECK_TYPES,
  type CheckRowIssue,
  LIMIT_FIELDS,
  type Translate,
  parseReading,
  validateCheckRows,
} from "./inspection-rules";

interface CheckRow {
  key: number;
  code: string;
  description: string;
  checkType: string;
  assetId: number | null;
  assetLabel: string | null;
  unit: string;
  normalMin: string;
  normalMax: string;
  warningMin: string;
  warningMax: string;
  requiresPhoto: boolean;
  commentOnException: boolean;
}

let rowKey = 0;
function nextKey(): number {
  rowKey += 1;
  return rowKey;
}

function emptyRow(): CheckRow {
  return {
    key: nextKey(),
    code: "",
    description: "",
    checkType: "pass_fail",
    assetId: null,
    assetLabel: null,
    unit: "",
    normalMin: "",
    normalMax: "",
    warningMin: "",
    warningMax: "",
    requiresPhoto: false,
    commentOnException: false,
  };
}

const num = (v: number | null) => (v != null ? String(v) : "");

function rowFrom(c: InspectionCheckpoint, assetLabel: string | null): CheckRow {
  return {
    key: nextKey(),
    code: c.checkpoint_code,
    description: c.description ?? "",
    checkType: c.check_type,
    assetId: c.asset_id,
    assetLabel,
    unit: c.measurement_unit ?? "",
    normalMin: num(c.normal_min),
    normalMax: num(c.normal_max),
    warningMin: num(c.warning_min),
    warningMax: num(c.warning_max),
    requiresPhoto: c.requires_photo,
    commentOnException: c.requires_comment_on_exception,
  };
}

async function assetLabels(ids: number[]): Promise<Map<number, string>> {
  const labels = new Map<number, string>();
  for (const id of new Set(ids)) {
    const asset = await getAssetByIdSilent(id);
    if (asset) labels.set(id, `${asset.asset_code} — ${asset.asset_name}`);
  }
  return labels;
}

interface InspectionTemplatesPanelProps {
  refreshKey: number;
  onEdit: (template: InspectionTemplate) => void;
  onCountChange?: (total: number | null) => void;
}

export function InspectionTemplatesPanel({
  refreshKey,
  onEdit,
  onCountChange,
}: InspectionTemplatesPanelProps) {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const all = await listInspectionTemplates();
      setTemplates(all);
      setError(null);
      onCountChange?.(all.length);
    } catch (err) {
      setError(err);
      onCountChange?.(null);
    } finally {
      setLoading(false);
    }
  }, [onCountChange]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const manage = can(P.INS_ADMIN);
  const columns = useMemo<ColumnDef<InspectionTemplate>[]>(
    () => [
      {
        id: "name",
        accessorFn: (x) => x.name,
        header: t("templates.columns.name"),
        cell: ({ row }) => (
          <div>
            <p className="font-medium text-text-primary">{row.original.name}</p>
            <p className="font-mono text-xs text-text-muted">{row.original.code}</p>
          </div>
        ),
      },
      {
        id: "duration",
        accessorFn: (x) => x.estimated_duration_minutes ?? 0,
        header: t("templates.duration"),
        cell: ({ row }) => (
          <span className="text-sm">
            {row.original.estimated_duration_minutes != null
              ? t("templates.minutes", { count: row.original.estimated_duration_minutes })
              : "—"}
          </span>
        ),
      },
      {
        id: "state",
        accessorFn: (x) => (x.is_active ? 1 : 0),
        header: t("templates.columns.state"),
        cell: ({ row }) => {
          const x = row.original;
          if (!x.is_active) return <StatusChip label={t("templates.inactive")} tone="neutral" />;
          if (x.current_version_id == null)
            return <StatusChip label={t("templates.unpublished")} tone="warning" />;
          return <StatusChip label={t("templates.published")} tone="success" />;
        },
      },
      ...(manage
        ? [
            {
              id: "edit",
              enableSorting: false,
              header: () => <span className="sr-only">{t("templates.newVersion")}</span>,
              cell: ({ row }: { row: { original: InspectionTemplate } }) => (
                <Button
                  variant="outline"
                  className="h-11"
                  aria-label={t("templates.newVersionNamed", { name: row.original.name })}
                  onClick={(e) => {
                    e.stopPropagation();
                    onEdit(row.original);
                  }}
                >
                  <Pencil className="mr-2 h-4 w-4" aria-hidden />
                  {t("templates.newVersion")}
                </Button>
              ),
            } satisfies ColumnDef<InspectionTemplate>,
          ]
        : []),
    ],
    [manage, onEdit, t],
  );

  if (error != null && templates.length === 0) {
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  }
  if (!loading && templates.length === 0) {
    return <EmptyState title={t("templates.empty")} description={t("templates.emptyHint")} />;
  }
  return (
    <div data-testid="inspection-template-list">
      <DataTable
        columns={columns}
        data={templates}
        pageSize={20}
        isLoading={loading}
        skeletonRows={4}
      />
    </div>
  );
}

function issueText(t: Translate, issue: CheckRowIssue) {
  return t(`templates.rowIssues.${issue.code}`, {
    field: issue.field ? t(`templates.${issue.field}`, {}) : "",
  });
}

interface TemplateBuilderDialogProps {
  /** `null`: closed; `"new"`: create a checklist; a template: publish its next version. */
  target: InspectionTemplate | "new" | null;
  onClose: () => void;
  onSaved: () => void;
}

export function TemplateBuilderDialog({ target, onClose, onSaved }: TemplateBuilderDialogProps) {
  const { t } = useTranslation("inspections");
  const template = target != null && target !== "new" ? target : null;
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [duration, setDuration] = useState("");
  const [requiresReview, setRequiresReview] = useState(false);
  const [rows, setRows] = useState<CheckRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [submitted, setSubmitted] = useState(false);
  const [stopsOpen, setStopsOpen] = useState(false);

  // A new version starts from the current one.
  useEffect(() => {
    if (target == null) return;
    setSubmitted(false);
    setLoadError(null);
    setCode(template?.code ?? "");
    setName(template?.name ?? "");
    setDuration(
      template?.estimated_duration_minutes != null
        ? String(template.estimated_duration_minutes)
        : "",
    );
    setRequiresReview(false);
    if (!template?.current_version_id) {
      setRows([emptyRow()]);
      setLoading(false);
      return;
    }
    const versionId = template.current_version_id;
    let cancelled = false;
    setLoading(true);
    void (async () => {
      try {
        const [versions, checks] = await Promise.all([
          listInspectionTemplateVersions({ template_id: template.id }),
          listInspectionCheckpoints({ template_version_id: versionId }),
        ]);
        const labels = await assetLabels(
          checks.map((c) => c.asset_id).filter((x): x is number => x != null),
        );
        if (cancelled) return;
        setRequiresReview(versions.find((v) => v.id === versionId)?.requires_review ?? false);
        setRows(
          checks.map((c) =>
            rowFrom(c, c.asset_id != null ? (labels.get(c.asset_id) ?? null) : null),
          ),
        );
      } catch (err) {
        if (!cancelled) setLoadError(err);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [target, template]);

  const issues = useMemo(() => validateCheckRows(rows), [rows]);
  const headerMissing = !template && (!code.trim() || !name.trim());
  const durationInvalid =
    duration.trim() !== "" &&
    !(Number.isInteger(parseReading(duration) ?? Number.NaN) && (parseReading(duration) ?? 0) > 0);

  const update = (key: number, patch: Partial<CheckRow>) =>
    setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const move = (i: number, delta: number) =>
    setRows((rs) => {
      const j = i + delta;
      if (j < 0 || j >= rs.length) return rs;
      const next = [...rs];
      const a = next[i];
      const b = next[j];
      if (!a || !b) return rs;
      next[i] = b;
      next[j] = a;
      return next;
    });

  const toDrafts = (): InspectionCheckpointDraft[] =>
    rows.map((r, i) => {
      const numeric = r.checkType === "numeric";
      const limit = (raw: string) => (numeric && raw.trim() !== "" ? parseReading(raw) : null);
      return {
        sequence_order: i + 1,
        asset_id: r.assetId,
        checkpoint_code: r.code.trim(),
        description: r.description.trim() || null,
        check_type: r.checkType,
        measurement_unit: numeric ? r.unit.trim() || null : null,
        normal_min: limit(r.normalMin),
        normal_max: limit(r.normalMax),
        warning_min: limit(r.warningMin),
        warning_max: limit(r.warningMax),
        requires_photo: r.requiresPhoto,
        requires_comment_on_exception: r.commentOnException,
      };
    });

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (rows.length === 0 || issues.size > 0 || headerMissing || durationInvalid) {
      // The first row with a problem is brought into view.
      const first = [...issues.keys()].sort((a, b) => a - b)[0];
      if (first != null)
        document.getElementById(`check-row-${first}`)?.scrollIntoView({ block: "center" });
      return;
    }
    setBusy(true);
    try {
      if (template) {
        await publishInspectionTemplateVersion({
          template_id: template.id,
          expected_row_version: template.row_version,
          requires_review: requiresReview,
          checkpoints: toDrafts(),
        });
      } else {
        await createInspectionTemplate({
          code: code.trim(),
          name: name.trim(),
          estimated_duration_minutes: duration.trim() === "" ? null : Number(duration),
          is_active: true,
          checkpoints: toDrafts(),
        });
      }
      onSaved();
    } catch {
      // The global IPC toast reports the failure (gate codes are translated); stay open.
    } finally {
      setBusy(false);
    }
  };

  const problemCount = submitted ? issues.size : 0;

  return (
    <Dialog open={target != null} onOpenChange={(o) => (!o ? onClose() : undefined)}>
      <DialogContent className="max-h-[90vh] max-w-3xl overflow-y-auto">
        <form onSubmit={(e) => void submit(e)} className="space-y-4" noValidate>
          <DialogHeader>
            <DialogTitle>
              {template
                ? t("templates.builderVersionTitle", { name: template.name })
                : t("templates.builderTitle")}
            </DialogTitle>
            <DialogDescription>{t("templates.builderIntro")}</DialogDescription>
          </DialogHeader>
          {!template ? (
            <div className="grid gap-3 md:grid-cols-3">
              <FormField
                name="tpl-code"
                label={t("templates.code")}
                required
                error={submitted && !code.trim() ? t("templates.required") : undefined}
              >
                <Input id="tpl-code" value={code} onChange={(e) => setCode(e.target.value)} />
              </FormField>
              <FormField
                name="tpl-name"
                label={t("templates.name")}
                required
                error={submitted && !name.trim() ? t("templates.required") : undefined}
              >
                <Input id="tpl-name" value={name} onChange={(e) => setName(e.target.value)} />
              </FormField>
              <FormField
                name="tpl-duration"
                label={t("templates.duration")}
                error={submitted && durationInvalid ? t("templates.durationInvalid") : undefined}
              >
                <Input
                  id="tpl-duration"
                  inputMode="numeric"
                  value={duration}
                  onChange={(e) => setDuration(e.target.value)}
                />
              </FormField>
            </div>
          ) : (
            <label className="flex min-h-11 items-center gap-2 text-sm">
              <Checkbox
                checked={requiresReview}
                onCheckedChange={(v) => setRequiresReview(v === true)}
              />
              {t("templates.requiresReview")}
            </label>
          )}

          <div className="flex flex-wrap items-center justify-between gap-2">
            <h4 className="text-sm font-semibold">{t("templates.checks")}</h4>
            <span className="text-xs text-text-muted">
              {t("templates.checkpoints", { count: rows.length })}
            </span>
          </div>

          {problemCount > 0 ? (
            <div
              role="alert"
              className="rounded-md border border-tone-danger-border bg-tone-danger-bg px-3 py-2 text-sm text-tone-danger-fg"
              data-testid="template-row-errors"
            >
              <p className="font-medium">{t("templates.rowsInvalid", { count: problemCount })}</p>
              <ul className="mt-1 list-disc space-y-0.5 pl-5">
                {[...issues.entries()]
                  .sort(([a], [b]) => a - b)
                  .map(([i, list]) => (
                    <li key={i}>
                      {t("templates.rowLabel", {
                        index: i + 1,
                        code: rows[i]?.code.trim() || t("templates.noCode"),
                      })}
                      {" : "}
                      {list.map((issue) => issueText(t, issue)).join(" ; ")}
                    </li>
                  ))}
              </ul>
            </div>
          ) : null}
          {submitted && rows.length === 0 ? (
            <p role="alert" className="text-sm text-tone-danger-fg">
              {t("templates.needOneCheck")}
            </p>
          ) : null}

          {loadError != null ? (
            <ErrorState error={loadError} />
          ) : loading ? (
            <LoadingState />
          ) : (
            <ol className="space-y-3">
              {rows.map((r, i) => {
                const rowIssues = submitted ? (issues.get(i) ?? []) : [];
                const fieldError = (f: string) => rowIssues.some((x) => x.field === f);
                return (
                  <li
                    key={r.key}
                    id={`check-row-${i}`}
                    className={
                      rowIssues.length > 0
                        ? "space-y-2 rounded-md border-2 border-tone-danger-border p-3"
                        : "space-y-2 rounded-md border border-surface-border p-3"
                    }
                    aria-invalid={rowIssues.length > 0}
                  >
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="w-6 text-sm font-semibold">{i + 1}</span>
                      <Input
                        aria-label={t("templates.checkCodeRow", { index: i + 1 })}
                        placeholder={t("templates.checkCode")}
                        className="h-11 max-w-48"
                        aria-invalid={rowIssues.some(
                          (x) => x.code === "codeRequired" || x.code === "codeDuplicate",
                        )}
                        value={r.code}
                        onChange={(e) => update(r.key, { code: e.target.value })}
                      />
                      <Select
                        value={r.checkType}
                        onValueChange={(v) => update(r.key, { checkType: v })}
                      >
                        <SelectTrigger
                          aria-label={t("templates.checkTypeRow", { index: i + 1 })}
                          className="h-11 max-w-56"
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          {CHECK_TYPES.map((ct) => (
                            <SelectItem key={ct} value={ct}>
                              {t(`templates.types.${ct}`)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                      <div className="ml-auto flex gap-1">
                        <Button
                          type="button"
                          variant="ghost"
                          className="h-11 w-11 p-0"
                          aria-label={t("templates.moveUpRow", { index: i + 1 })}
                          disabled={i === 0}
                          onClick={() => move(i, -1)}
                        >
                          <ArrowUp className="h-4 w-4" aria-hidden />
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          className="h-11 w-11 p-0"
                          aria-label={t("templates.moveDownRow", { index: i + 1 })}
                          disabled={i === rows.length - 1}
                          onClick={() => move(i, 1)}
                        >
                          <ArrowDown className="h-4 w-4" aria-hidden />
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          className="h-11 w-11 p-0"
                          aria-label={t("templates.removeRow", { index: i + 1 })}
                          onClick={() => setRows((rs) => rs.filter((x) => x.key !== r.key))}
                        >
                          <Trash2 className="h-4 w-4" aria-hidden />
                        </Button>
                      </div>
                    </div>
                    <Textarea
                      aria-label={t("templates.checkDescription")}
                      placeholder={t("templates.checkDescription")}
                      rows={2}
                      value={r.description}
                      onChange={(e) => update(r.key, { description: e.target.value })}
                    />
                    {r.assetId != null && r.assetLabel ? (
                      <div className="flex items-center gap-2 text-sm">
                        <span>{r.assetLabel}</span>
                        <Button
                          type="button"
                          variant="ghost"
                          className="h-11"
                          onClick={() => update(r.key, { assetId: null, assetLabel: null })}
                        >
                          {t("schedule.clearAssignee")}
                        </Button>
                      </div>
                    ) : (
                      <AssetPicker
                        value={null}
                        onChange={(a) =>
                          update(r.key, {
                            assetId: a?.id ?? null,
                            assetLabel: a ? `${a.asset_code} — ${a.asset_name}` : null,
                          })
                        }
                      />
                    )}
                    {r.checkType === "numeric" ? (
                      <div className="grid grid-cols-2 gap-2 md:grid-cols-5">
                        <Input
                          aria-label={t("templates.unit")}
                          placeholder={t("templates.unit")}
                          className="h-11"
                          value={r.unit}
                          onChange={(e) => update(r.key, { unit: e.target.value })}
                        />
                        {LIMIT_FIELDS.map((f) => (
                          <Input
                            key={f}
                            aria-label={t(`templates.${f}`)}
                            placeholder={t(`templates.${f}`)}
                            inputMode="decimal"
                            className="h-11"
                            aria-invalid={fieldError(f)}
                            value={r[f]}
                            onChange={(e) => update(r.key, { [f]: e.target.value })}
                          />
                        ))}
                      </div>
                    ) : null}
                    <div className="flex flex-wrap gap-4 text-sm">
                      <label className="flex min-h-11 items-center gap-2">
                        <Checkbox
                          checked={r.requiresPhoto}
                          onCheckedChange={(v) => update(r.key, { requiresPhoto: v === true })}
                        />
                        {t("templates.requiresPhoto")}
                      </label>
                      <label className="flex min-h-11 items-center gap-2">
                        <Checkbox
                          checked={r.commentOnException}
                          onCheckedChange={(v) => update(r.key, { commentOnException: v === true })}
                        />
                        {t("templates.commentOnException")}
                      </label>
                    </div>
                    {rowIssues.length > 0 ? (
                      <ul
                        className="space-y-0.5 text-xs text-tone-danger-fg"
                        data-testid="row-issues"
                      >
                        {rowIssues.map((issue) => (
                          <li key={`${issue.code}-${issue.field ?? ""}`}>{issueText(t, issue)}</li>
                        ))}
                      </ul>
                    ) : null}
                  </li>
                );
              })}
            </ol>
          )}
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => setRows((rs) => [...rs, emptyRow()])}
            >
              <Plus className="mr-2 h-4 w-4" aria-hidden />
              {t("templates.addCheck")}
            </Button>
            <Button
              type="button"
              variant="outline"
              className="h-11"
              onClick={() => setStopsOpen(true)}
            >
              <Copy className="mr-2 h-4 w-4" aria-hidden />
              {t("templates.addStop")}
            </Button>
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" className="h-11" onClick={onClose}>
              {t("followUp.cancel")}
            </Button>
            <Button type="submit" className="h-11" disabled={busy || loading}>
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
              {template ? t("templates.publish") : t("templates.save")}
            </Button>
          </DialogFooter>
        </form>
        <AddStopDialog
          open={stopsOpen}
          excludeTemplateId={template?.id ?? null}
          onClose={() => setStopsOpen(false)}
          onAdd={(added) => {
            setRows((rs) => [...rs.filter((r) => r.code.trim() !== "" || rs.length > 1), ...added]);
            setStopsOpen(false);
          }}
        />
      </DialogContent>
    </Dialog>
  );
}

interface AddStopDialogProps {
  open: boolean;
  excludeTemplateId: number | null;
  onClose: () => void;
  onAdd: (rows: CheckRow[]) => void;
}

/**
 * Route building: inserts the checks of another checklist, optionally for a given equipment
 * (each check is then bound to it and its code prefixed with the equipment code, so a standard
 * "pump" checklist can be walked on P-101, P-102 … in one route).
 */
function AddStopDialog({ open, excludeTemplateId, onClose, onAdd }: AddStopDialogProps) {
  const { t } = useTranslation("inspections");
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [sourceId, setSourceId] = useState("");
  const [asset, setAsset] = useState<{ id: number; asset_code: string; asset_name: string } | null>(
    null,
  );
  const [loadError, setLoadError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setSourceId("");
    setAsset(null);
    setLoadError(null);
    listInspectionTemplates()
      .then((all) =>
        setTemplates(all.filter((x) => x.current_version_id != null && x.id !== excludeTemplateId)),
      )
      .catch((err: unknown) => setLoadError(err));
  }, [open, excludeTemplateId]);

  const add = async () => {
    const source = templates.find((x) => String(x.id) === sourceId);
    if (!source?.current_version_id) return;
    setBusy(true);
    try {
      const checks = await listInspectionCheckpoints({
        template_version_id: source.current_version_id,
      });
      const labels = asset
        ? new Map<number, string>()
        : await assetLabels(checks.map((c) => c.asset_id).filter((x): x is number => x != null));
      onAdd(
        checks.map((c) => {
          const row = rowFrom(c, c.asset_id != null ? (labels.get(c.asset_id) ?? null) : null);
          if (!asset) return row;
          return {
            ...row,
            code: `${asset.asset_code}-${c.checkpoint_code}`,
            assetId: asset.id,
            assetLabel: `${asset.asset_code} — ${asset.asset_name}`,
          };
        }),
      );
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => (!o ? onClose() : undefined)}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t("templates.addStopTitle")}</DialogTitle>
          <DialogDescription>{t("templates.addStopIntro")}</DialogDescription>
        </DialogHeader>
        {loadError != null ? <ErrorState error={loadError} /> : null}
        <FormField name="stop-source" label={t("templates.addStopSource")} required>
          <Select value={sourceId} onValueChange={setSourceId}>
            <SelectTrigger id="stop-source" className="h-11">
              <SelectValue placeholder={t("schedule.templatePlaceholder")} />
            </SelectTrigger>
            <SelectContent>
              {templates.map((x) => (
                <SelectItem key={x.id} value={String(x.id)}>
                  {x.name} ({x.code})
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
        <FormField
          name="stop-asset"
          label={t("templates.addStopAsset")}
          description={t("templates.addStopAssetHint")}
        >
          {asset ? (
            <div className="flex min-h-11 items-center justify-between rounded-md border border-surface-border px-3 text-sm">
              <span>
                <span className="font-mono">{asset.asset_code}</span> {asset.asset_name}
              </span>
              <Button type="button" variant="ghost" className="h-11" onClick={() => setAsset(null)}>
                {t("schedule.clearAssignee")}
              </Button>
            </div>
          ) : (
            <AssetPicker
              id="stop-asset"
              value={null}
              onChange={(a) =>
                setAsset(
                  a ? { id: a.id, asset_code: a.asset_code, asset_name: a.asset_name } : null,
                )
              }
            />
          )}
        </FormField>
        <DialogFooter>
          <Button type="button" variant="outline" className="h-11" onClick={onClose}>
            {t("followUp.cancel")}
          </Button>
          <Button
            type="button"
            className="h-11"
            disabled={!sourceId || busy}
            onClick={() => void add()}
          >
            {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
            {t("templates.addStopSubmit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
