/**
 * Checklist templates: list, create and publish a new version (versions are never edited in
 * place, PRD §6.25). Checks pick their equipment with the asset picker; no ids are typed.
 */
import { ArrowDown, ArrowUp, Loader2, Pencil, Plus, Trash2 } from "lucide-react";
import { type FormEvent, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { AssetPicker } from "@/components/assets/AssetPicker";
import {
  Badge,
  Button,
  Dialog,
  DialogContent,
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
import { usePermissions } from "@/hooks/use-permissions";
import { getAssetByIdSilent } from "@/services/asset-service";
import {
  createInspectionTemplate,
  listInspectionCheckpoints,
  listInspectionTemplateVersions,
  listInspectionTemplates,
  publishInspectionTemplateVersion,
} from "@/services/inspection-service";
import { toErrorMessage } from "@/utils/errors";
import type { InspectionCheckpointDraft, InspectionTemplate } from "@shared/ipc-types";

import { CHECK_TYPES, parseReading } from "./inspection-rules";

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
function emptyRow(): CheckRow {
  rowKey += 1;
  return {
    key: rowKey,
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

const LIMIT_FIELDS = ["normalMin", "normalMax", "warningMin", "warningMax"] as const;

function optNumber(raw: string): number | null | "invalid" {
  if (raw.trim() === "") return null;
  return parseReading(raw) ?? "invalid";
}

export function InspectionTemplatesPanel() {
  const { t } = useTranslation("inspections");
  const { can } = usePermissions();
  const [templates, setTemplates] = useState<InspectionTemplate[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [builder, setBuilder] = useState<{ template: InspectionTemplate | null } | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setTemplates(await listInspectionTemplates());
      setError(null);
    } catch (err) {
      setError(t("errors.load", { message: toErrorMessage(err) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    void load();
  }, [load]);

  const manage = can("ins.admin");

  return (
    <div className="space-y-4">
      {manage ? (
        <Button size="lg" className="min-h-12" onClick={() => setBuilder({ template: null })}>
          <Plus className="mr-2 h-5 w-5" />
          {t("templates.new")}
        </Button>
      ) : null}
      {error ? (
        <p role="alert" className="text-sm text-status-danger">
          {error}
        </p>
      ) : null}
      {loading ? (
        <Loader2 className="h-5 w-5 animate-spin text-text-muted" />
      ) : templates.length === 0 ? (
        <p className="text-sm text-text-muted">{t("templates.empty")}</p>
      ) : (
        <ul className="divide-y divide-surface-border rounded-lg border border-surface-border">
          {templates.map((tpl) => (
            <li key={tpl.id} className="flex flex-wrap items-center gap-3 p-3">
              <div className="min-w-0 flex-1">
                <p className="font-medium">
                  {tpl.name} <span className="text-xs text-text-muted">({tpl.code})</span>
                </p>
                {tpl.estimated_duration_minutes != null ? (
                  <p className="text-xs text-text-muted">
                    {t("templates.duration")} : {tpl.estimated_duration_minutes}
                  </p>
                ) : null}
              </div>
              {!tpl.is_active ? <Badge variant="outline">{t("templates.inactive")}</Badge> : null}
              {manage ? (
                <Button
                  variant="outline"
                  size="lg"
                  className="min-h-12"
                  onClick={() => setBuilder({ template: tpl })}
                >
                  <Pencil className="mr-2 h-4 w-4" />
                  {t("templates.newVersion")}
                </Button>
              ) : null}
            </li>
          ))}
        </ul>
      )}
      {builder ? (
        <TemplateBuilderDialog
          template={builder.template}
          onClose={() => setBuilder(null)}
          onSaved={() => {
            setBuilder(null);
            void load();
          }}
        />
      ) : null}
    </div>
  );
}

interface TemplateBuilderDialogProps {
  template: InspectionTemplate | null;
  onClose: () => void;
  onSaved: () => void;
}

function TemplateBuilderDialog({ template, onClose, onSaved }: TemplateBuilderDialogProps) {
  const { t } = useTranslation("inspections");
  const [code, setCode] = useState(template?.code ?? "");
  const [name, setName] = useState(template?.name ?? "");
  const [duration, setDuration] = useState(
    template?.estimated_duration_minutes != null ? String(template.estimated_duration_minutes) : "",
  );
  const [requiresReview, setRequiresReview] = useState(false);
  const [rows, setRows] = useState<CheckRow[]>(template ? [] : [emptyRow()]);
  const [loading, setLoading] = useState(template != null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitted, setSubmitted] = useState(false);

  // A new version starts from the current one.
  useEffect(() => {
    if (!template?.current_version_id) {
      setLoading(false);
      return;
    }
    const versionId = template.current_version_id;
    let cancelled = false;
    void (async () => {
      try {
        const [versions, checks] = await Promise.all([
          listInspectionTemplateVersions({ template_id: template.id }),
          listInspectionCheckpoints({ template_version_id: versionId }),
        ]);
        const labels = new Map<number, string>();
        for (const id of new Set(
          checks.map((c) => c.asset_id).filter((x): x is number => x != null),
        )) {
          const asset = await getAssetByIdSilent(id);
          if (asset) labels.set(id, `${asset.asset_code} — ${asset.asset_name}`);
        }
        if (cancelled) return;
        setRequiresReview(versions.find((v) => v.id === versionId)?.requires_review ?? false);
        setRows(
          checks.map((c) => {
            rowKey += 1;
            return {
              key: rowKey,
              code: c.checkpoint_code,
              description: c.description ?? "",
              checkType: c.check_type,
              assetId: c.asset_id,
              assetLabel: c.asset_id != null ? (labels.get(c.asset_id) ?? null) : null,
              unit: c.measurement_unit ?? "",
              normalMin: c.normal_min != null ? String(c.normal_min) : "",
              normalMax: c.normal_max != null ? String(c.normal_max) : "",
              warningMin: c.warning_min != null ? String(c.warning_min) : "",
              warningMax: c.warning_max != null ? String(c.warning_max) : "",
              requiresPhoto: c.requires_photo,
              commentOnException: c.requires_comment_on_exception,
            };
          }),
        );
      } catch (err) {
        if (!cancelled) setError(t("errors.load", { message: toErrorMessage(err) }));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [template, t]);

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

  const toDrafts = (): InspectionCheckpointDraft[] | null => {
    const drafts: InspectionCheckpointDraft[] = [];
    for (const [i, r] of rows.entries()) {
      if (!r.code.trim()) return null;
      const limits: Partial<Record<(typeof LIMIT_FIELDS)[number], number | null>> = {};
      if (r.checkType === "numeric") {
        for (const f of LIMIT_FIELDS) {
          const v = optNumber(r[f]);
          if (v === "invalid") return null;
          limits[f] = v;
        }
      }
      drafts.push({
        sequence_order: i + 1,
        asset_id: r.assetId,
        checkpoint_code: r.code.trim(),
        description: r.description.trim() || null,
        check_type: r.checkType,
        measurement_unit: r.checkType === "numeric" ? r.unit.trim() || null : null,
        normal_min: limits.normalMin ?? null,
        normal_max: limits.normalMax ?? null,
        warning_min: limits.warningMin ?? null,
        warning_max: limits.warningMax ?? null,
        requires_photo: r.requiresPhoto,
        requires_comment_on_exception: r.commentOnException,
      });
    }
    return drafts;
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitted(true);
    if (rows.length === 0) {
      setError(t("templates.needOneCheck"));
      return;
    }
    const drafts = toDrafts();
    if (!drafts || (!template && (!code.trim() || !name.trim()))) return;
    const minutes = duration.trim() === "" ? null : Number.parseInt(duration, 10);
    setBusy(true);
    setError(null);
    try {
      if (template) {
        await publishInspectionTemplateVersion({
          template_id: template.id,
          expected_row_version: template.row_version,
          requires_review: requiresReview,
          checkpoints: drafts,
        });
      } else {
        await createInspectionTemplate({
          code: code.trim(),
          name: name.trim(),
          estimated_duration_minutes: Number.isFinite(minutes) ? minutes : null,
          is_active: true,
          checkpoints: drafts,
        });
      }
      onSaved();
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open onOpenChange={(o) => (!o ? onClose() : undefined)}>
      <DialogContent className="max-h-[90vh] max-w-3xl overflow-y-auto">
        <form onSubmit={(e) => void submit(e)} className="space-y-4">
          <DialogHeader>
            <DialogTitle>
              {template
                ? t("templates.builderVersionTitle", { name: template.name })
                : t("templates.builderTitle")}
            </DialogTitle>
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
              <FormField name="tpl-duration" label={t("templates.duration")}>
                <Input
                  id="tpl-duration"
                  inputMode="numeric"
                  value={duration}
                  onChange={(e) => setDuration(e.target.value)}
                />
              </FormField>
            </div>
          ) : (
            <label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={requiresReview}
                onCheckedChange={(v) => setRequiresReview(v === true)}
              />
              {t("templates.requiresReview")}
            </label>
          )}

          <h4 className="text-sm font-semibold">{t("templates.checks")}</h4>
          {loading ? (
            <Loader2 className="h-5 w-5 animate-spin" />
          ) : (
            <ol className="space-y-3">
              {rows.map((r, i) => (
                <li key={r.key} className="space-y-2 rounded-md border border-surface-border p-3">
                  <div className="flex items-center gap-2">
                    <span className="w-6 text-sm font-semibold">{i + 1}</span>
                    <Input
                      aria-label={t("templates.checkCode")}
                      placeholder={t("templates.checkCode")}
                      className="max-w-48"
                      value={r.code}
                      onChange={(e) => update(r.key, { code: e.target.value })}
                    />
                    <Select
                      value={r.checkType}
                      onValueChange={(v) => update(r.key, { checkType: v })}
                    >
                      <SelectTrigger aria-label={t("templates.checkType")} className="max-w-56">
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
                        size="icon"
                        aria-label={t("templates.moveUp")}
                        onClick={() => move(i, -1)}
                      >
                        <ArrowUp className="h-4 w-4" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        aria-label={t("templates.moveDown")}
                        onClick={() => move(i, 1)}
                      >
                        <ArrowDown className="h-4 w-4" />
                      </Button>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon"
                        aria-label={t("templates.remove")}
                        onClick={() => setRows((rs) => rs.filter((x) => x.key !== r.key))}
                      >
                        <Trash2 className="h-4 w-4" />
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
                        size="sm"
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
                        value={r.unit}
                        onChange={(e) => update(r.key, { unit: e.target.value })}
                      />
                      {LIMIT_FIELDS.map((f) => (
                        <div key={f}>
                          <Input
                            aria-label={t(`templates.${f}`)}
                            placeholder={t(`templates.${f}`)}
                            inputMode="decimal"
                            value={r[f]}
                            onChange={(e) => update(r.key, { [f]: e.target.value })}
                          />
                          {optNumber(r[f]) === "invalid" ? (
                            <p className="text-xs text-status-danger">
                              {t("templates.invalidNumber")}
                            </p>
                          ) : null}
                        </div>
                      ))}
                    </div>
                  ) : null}
                  <div className="flex flex-wrap gap-4 text-sm">
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={r.requiresPhoto}
                        onCheckedChange={(v) => update(r.key, { requiresPhoto: v === true })}
                      />
                      {t("templates.requiresPhoto")}
                    </label>
                    <label className="flex items-center gap-2">
                      <Checkbox
                        checked={r.commentOnException}
                        onCheckedChange={(v) => update(r.key, { commentOnException: v === true })}
                      />
                      {t("templates.commentOnException")}
                    </label>
                  </div>
                </li>
              ))}
            </ol>
          )}
          <Button
            type="button"
            variant="outline"
            onClick={() => setRows((rs) => [...rs, emptyRow()])}
          >
            <Plus className="mr-2 h-4 w-4" />
            {t("templates.addCheck")}
          </Button>
          {error ? (
            <p role="alert" className="text-sm text-status-danger">
              {error}
            </p>
          ) : null}
          <DialogFooter>
            <Button type="submit" size="lg" disabled={busy || loading}>
              {busy ? <Loader2 className="mr-2 h-4 w-4 animate-spin" /> : null}
              {template ? t("templates.publish") : t("templates.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
