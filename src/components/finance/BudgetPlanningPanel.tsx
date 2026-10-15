/**
 * Budget planning: cost centres, the budget versions of the year with their approval lifecycle
 * (draft → submitted → approved → frozen → closed; approval needs `fin.approve` + step-up) and the
 * budget lines of a version (cost centre × category × month or whole year). Only a draft version
 * can be edited; a reforecast is created as a successor that copies the lines.
 */
import { Pencil, Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { parseAmount } from "@/components/finance/WoCostEstimateDialog";
import { VERSION_STATUS_TONE } from "@/components/finance/finance-tone";
import { useFinanceCatalogs } from "@/components/finance/use-finance-catalogs";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { EmptyState } from "@/components/ui/empty-state";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { StatusChip } from "@/components/ui/status-chip";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { toneOf } from "@/design-system/tone";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import { cn } from "@/lib/utils";
import {
  createBudgetLine,
  createBudgetSuccessorVersion,
  createBudgetVersion,
  createCostCenter,
  listBudgetLines,
  listBudgetVersions,
  listCostCenters,
  transitionBudgetVersionLifecycle,
  updateBudgetLine,
  updateCostCenter,
} from "@/services/budget-service";
import { listOrgTree } from "@/services/org-node-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { BudgetLine, BudgetVersion, CostCenter } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const NONE = "__none__";
const ANNUAL = "__annual__";
const SCENARIOS = ["original", "approved", "reforecast", "what_if"] as const;

/** Next lifecycle steps a user may take from a version status (backend `valid_version_transition`). */
export const NEXT_VERSION_STATUS: Record<string, string[]> = {
  draft: ["submitted"],
  submitted: ["approved"],
  approved: ["frozen", "closed"],
  frozen: ["closed"],
};

const TH = "px-3 py-2 text-start font-semibold";

export interface BudgetPlanningPanelProps {
  fiscalYear: number;
  /** Version shown in the cockpit, selected by default. */
  selectedVersionId: number | null;
  onSelectVersion: (id: number | null) => void;
  onChanged: () => void;
}

export function BudgetPlanningPanel({
  fiscalYear,
  selectedVersionId,
  onSelectVersion,
  onChanged,
}: BudgetPlanningPanelProps) {
  const { t } = useTranslation("budget");
  const { can } = usePermissions();
  const canBudget = can(P.FIN_BUDGET) || can(P.FIN_MANAGE);
  const [centers, setCenters] = useState<CostCenter[]>([]);
  const [versions, setVersions] = useState<BudgetVersion[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [cc, vs] = await Promise.all([
        listCostCenters({ include_inactive: true }),
        listBudgetVersions({ fiscal_year: fiscalYear }),
      ]);
      setCenters(cc);
      setVersions(vs);
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, [fiscalYear]);

  useEffect(() => {
    void load();
  }, [load]);

  const refresh = useCallback(async () => {
    await load();
    onChanged();
  }, [load, onChanged]);

  if (loading && centers.length === 0 && versions.length === 0) return <LoadingState />;
  if (error) {
    return <ErrorState error={error} title={t("planning.loadError")} onRetry={() => void load()} />;
  }

  const version = versions.find((v) => v.id === selectedVersionId) ?? null;

  return (
    <div className="space-y-6">
      <CostCentersSection centers={centers} canEdit={canBudget} onChanged={refresh} />
      <VersionsSection
        fiscalYear={fiscalYear}
        versions={versions}
        selectedVersionId={selectedVersionId}
        onSelect={onSelectVersion}
        canEdit={canBudget}
        onChanged={refresh}
      />
      {version ? (
        <LinesSection
          version={version}
          centers={centers.filter((c) => c.is_active === 1)}
          canEdit={canBudget && version.status === "draft"}
          onChanged={refresh}
        />
      ) : null}
    </div>
  );
}

// ── Cost centres ────────────────────────────────────────────────────────────────────────

function CostCentersSection({
  centers,
  canEdit,
  onChanged,
}: {
  centers: CostCenter[];
  canEdit: boolean;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation("budget");
  const [editing, setEditing] = useState<CostCenter | "new" | null>(null);

  return (
    <section aria-labelledby="cc-title" className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 id="cc-title" className="text-sm font-semibold text-text-primary">
            {t("planning.costCenters")}
          </h3>
          <p className="text-xs text-text-muted">{t("planning.costCentersHint")}</p>
        </div>
        {canEdit ? (
          <Button size="sm" className="gap-1.5" onClick={() => setEditing("new")}>
            <Plus className="h-3.5 w-3.5" aria-hidden />
            {t("planning.newCostCenter")}
          </Button>
        ) : null}
      </div>
      {centers.length === 0 ? (
        <EmptyState
          title={t("planning.noCostCenters")}
          description={t("planning.noCostCentersBody")}
        />
      ) : (
        <div className="overflow-x-auto rounded-lg border border-surface-border">
          <table className="w-full text-sm">
            <thead className="bg-surface-2 text-xs uppercase tracking-wide text-text-secondary">
              <tr>
                <th scope="col" className={TH}>
                  {t("planning.col.code")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.name")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.orgNode")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.parent")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.state")}
                </th>
                <th scope="col" className="px-3 py-2">
                  <span className="sr-only">{t("planning.col.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {centers.map((c) => (
                <tr key={c.id} className="border-t border-surface-border">
                  <td className="px-3 py-2 font-mono">{c.code}</td>
                  <td className="px-3 py-2">{c.name}</td>
                  <td className="px-3 py-2 text-text-secondary">{c.entity_name ?? "—"}</td>
                  <td className="px-3 py-2 text-text-secondary">
                    {c.parent_cost_center_code ?? "—"}
                  </td>
                  <td className="px-3 py-2">
                    <StatusChip
                      tone={c.is_active === 1 ? "success" : "neutral"}
                      label={c.is_active === 1 ? t("planning.active") : t("planning.inactive")}
                    />
                  </td>
                  <td className="px-3 py-2 text-end">
                    {canEdit ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-7 w-7 p-0"
                        aria-label={t("planning.editCostCenter", { code: c.code })}
                        onClick={() => setEditing(c)}
                      >
                        <Pencil className="h-3.5 w-3.5" aria-hidden />
                      </Button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {editing ? (
        <CostCenterDialog
          center={editing === "new" ? null : editing}
          centers={centers}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await onChanged();
          }}
        />
      ) : null}
    </section>
  );
}

function CostCenterDialog({
  center,
  centers,
  onClose,
  onSaved,
}: {
  center: CostCenter | null;
  centers: CostCenter[];
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation("budget");
  const [code, setCode] = useState(center?.code ?? "");
  const [name, setName] = useState(center?.name ?? "");
  const [entity, setEntity] = useState(center?.entity_id != null ? String(center.entity_id) : NONE);
  const [parent, setParent] = useState(
    center?.parent_cost_center_id != null ? String(center.parent_cost_center_id) : NONE,
  );
  const [active, setActive] = useState(center ? center.is_active === 1 : true);
  const [nodes, setNodes] = useState<{ id: number; label: string }[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    listOrgTree()
      .then((rows) =>
        setNodes(
          rows
            .filter((r) => r.can_carry_cost_center)
            .map((r) => ({ id: r.node.id, label: `${r.node.code} — ${r.node.name}` })),
        ),
      )
      .catch((err: unknown) => setError(err));
  }, []);

  const valid = code.trim().length > 0 && name.trim().length > 0;
  const submit = async () => {
    if (!valid || saving) return;
    setSaving(true);
    setError(null);
    const payload = {
      code: code.trim(),
      name: name.trim(),
      entity_id: entity === NONE ? null : Number(entity),
      parent_cost_center_id: parent === NONE ? null : Number(parent),
      is_active: active,
    };
    try {
      if (center) {
        await updateCostCenter(center.id, center.row_version, payload);
      } else {
        await createCostCenter(payload);
      }
      pushAppToast({
        title: t("planning.costCenterSaved", { code: payload.code }),
        variant: "success",
      });
      await onSaved();
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => (open ? undefined : onClose())}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>
            {center
              ? t("planning.editCostCenter", { code: center.code })
              : t("planning.newCostCenter")}
          </DialogTitle>
          <DialogDescription>{t("planning.costCenterDialogHint")}</DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="grid grid-cols-3 gap-3">
            <div className="space-y-1">
              <Label htmlFor="cc-code">{t("planning.col.code")}</Label>
              <Input id="cc-code" value={code} onChange={(e) => setCode(e.target.value)} required />
            </div>
            <div className="col-span-2 space-y-1">
              <Label htmlFor="cc-name">{t("planning.col.name")}</Label>
              <Input id="cc-name" value={name} onChange={(e) => setName(e.target.value)} required />
            </div>
          </div>
          <div className="space-y-1">
            <Label htmlFor="cc-node">{t("planning.col.orgNode")}</Label>
            <Select value={entity} onValueChange={setEntity}>
              <SelectTrigger id="cc-node">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>{t("planning.none")}</SelectItem>
                {nodes.map((n) => (
                  <SelectItem key={n.id} value={String(n.id)}>
                    {n.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <p className="text-xs text-text-muted">{t("planning.orgNodeHint")}</p>
          </div>
          <div className="space-y-1">
            <Label htmlFor="cc-parent">{t("planning.col.parent")}</Label>
            <Select value={parent} onValueChange={setParent}>
              <SelectTrigger id="cc-parent">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NONE}>{t("planning.none")}</SelectItem>
                {centers
                  .filter((c) => c.id !== center?.id)
                  .map((c) => (
                    <SelectItem key={c.id} value={String(c.id)}>
                      {c.code} — {c.name}
                    </SelectItem>
                  ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex items-center gap-2">
            <Switch id="cc-active" checked={active} onCheckedChange={setActive} />
            <Label htmlFor="cc-active">{t("planning.active")}</Label>
          </div>
          {error ? <ErrorState error={error} title={t("planning.saveError")} /> : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              {t("actions.cancel")}
            </Button>
            <Button type="submit" disabled={!valid || saving}>
              {saving ? t("actions.saving") : t("actions.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

// ── Versions ────────────────────────────────────────────────────────────────────────────

function VersionsSection({
  fiscalYear,
  versions,
  selectedVersionId,
  onSelect,
  canEdit,
  onChanged,
}: {
  fiscalYear: number;
  versions: BudgetVersion[];
  selectedVersionId: number | null;
  onSelect: (id: number | null) => void;
  canEdit: boolean;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation("budget");
  const { currencyCode } = useFormatters();
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const [creating, setCreating] = useState(false);
  const [scenario, setScenario] = useState<string>("original");
  const [title, setTitle] = useState("");
  const [busy, setBusy] = useState<number | null>(null);
  const [error, setError] = useState<unknown>(null);

  const transition = async (v: BudgetVersion, next: string) => {
    setBusy(v.id);
    setError(null);
    const run = () =>
      transitionBudgetVersionLifecycle({
        version_id: v.id,
        expected_row_version: v.row_version,
        next_status: next,
      });
    try {
      await (next === "approved" ? withStepUp(run) : run());
      pushAppToast({
        title: t("planning.versionMoved", { status: t(`versionStatus.${next}`) }),
        variant: "success",
      });
      await onChanged();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(null);
    }
  };

  const successor = async (v: BudgetVersion) => {
    setBusy(v.id);
    setError(null);
    try {
      const created = await createBudgetSuccessorVersion({
        source_version_id: v.id,
        scenario_type: "reforecast",
        title: t("planning.reforecastTitle", { title: v.title ?? `${fiscalYear}` }),
      });
      onSelect(created.id);
      await onChanged();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(null);
    }
  };

  const create = async () => {
    setError(null);
    try {
      const created = await createBudgetVersion({
        fiscal_year: fiscalYear,
        scenario_type: scenario,
        currency_code: currencyCode,
        title: title.trim() || null,
      });
      setCreating(false);
      setTitle("");
      onSelect(created.id);
      await onChanged();
    } catch (err) {
      setError(err);
    }
  };

  return (
    <section aria-labelledby="versions-title" className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 id="versions-title" className="text-sm font-semibold text-text-primary">
            {t("planning.versions", { year: fiscalYear })}
          </h3>
          <p className="text-xs text-text-muted">{t("planning.versionsHint")}</p>
        </div>
        {canEdit ? (
          <Button size="sm" className="gap-1.5" onClick={() => setCreating(true)}>
            <Plus className="h-3.5 w-3.5" aria-hidden />
            {t("planning.newVersion")}
          </Button>
        ) : null}
      </div>
      {error ? <ErrorState error={error} title={t("planning.saveError")} /> : null}
      {versions.length === 0 ? (
        <EmptyState
          title={t("planning.noVersions", { year: fiscalYear })}
          description={t("planning.noVersionsBody")}
        />
      ) : (
        <ul className="divide-y divide-surface-border rounded-lg border border-surface-border">
          {versions.map((v) => (
            <li
              key={v.id}
              className={cn(
                "flex flex-wrap items-center gap-3 px-3 py-2",
                v.id === selectedVersionId && "bg-surface-2",
              )}
            >
              <button
                type="button"
                className="min-w-0 flex-1 text-start"
                aria-pressed={v.id === selectedVersionId}
                onClick={() => onSelect(v.id)}
              >
                <div className="font-medium text-text-primary">
                  {v.title ?? t("planning.untitled")} · v{v.version_no}
                </div>
                <div className="text-xs text-text-muted">
                  {t(`scenario.${v.scenario_type}`, { defaultValue: v.scenario_type })} ·{" "}
                  {v.currency_code}
                </div>
              </button>
              <StatusChip
                tone={toneOf(VERSION_STATUS_TONE, v.status)}
                label={t(`versionStatus.${v.status}`)}
              />
              <div className="flex flex-wrap gap-1">
                {(NEXT_VERSION_STATUS[v.status] ?? []).map((next) => {
                  const allowed = next === "approved" ? can(P.FIN_APPROVE) : canEdit;
                  if (!allowed) return null;
                  return (
                    <Button
                      key={next}
                      size="sm"
                      variant="outline"
                      className="h-7 text-xs"
                      disabled={busy === v.id}
                      onClick={() => void transition(v, next)}
                    >
                      {t(`versionAction.${next}`)}
                    </Button>
                  );
                })}
                {canEdit && (v.status === "approved" || v.status === "frozen") ? (
                  <Button
                    size="sm"
                    variant="outline"
                    className="h-7 text-xs"
                    disabled={busy === v.id}
                    onClick={() => void successor(v)}
                  >
                    {t("planning.reforecast")}
                  </Button>
                ) : null}
              </div>
            </li>
          ))}
        </ul>
      )}
      <Dialog open={creating} onOpenChange={setCreating}>
        <DialogContent className="max-w-md">
          <DialogHeader>
            <DialogTitle>{t("planning.newVersion")}</DialogTitle>
            <DialogDescription>
              {t("planning.newVersionHint", { year: fiscalYear, currency: currencyCode })}
            </DialogDescription>
          </DialogHeader>
          <div className="space-y-3">
            <div className="space-y-1">
              <Label htmlFor="v-scenario">{t("planning.scenario")}</Label>
              <Select value={scenario} onValueChange={setScenario}>
                <SelectTrigger id="v-scenario">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SCENARIOS.map((s) => (
                    <SelectItem key={s} value={s}>
                      {t(`scenario.${s}`)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="v-title">{t("planning.versionTitle")}</Label>
              <Input id="v-title" value={title} onChange={(e) => setTitle(e.target.value)} />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setCreating(false)}>
              {t("actions.cancel")}
            </Button>
            <Button onClick={() => void create()}>{t("actions.create")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
      {StepUpDialogElement}
    </section>
  );
}

// ── Lines ───────────────────────────────────────────────────────────────────────────────

function LinesSection({
  version,
  centers,
  canEdit,
  onChanged,
}: {
  version: BudgetVersion;
  centers: CostCenter[];
  canEdit: boolean;
  onChanged: () => Promise<void>;
}) {
  const { t } = useTranslation("budget");
  const { categoryLabel, periodLabel } = useFinanceCatalogs();
  const { formatCurrency } = useFormatters();
  const [lines, setLines] = useState<BudgetLine[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [editing, setEditing] = useState<BudgetLine | "new" | null>(null);

  const load = useCallback(async () => {
    try {
      setLines(await listBudgetLines({ budget_version_id: version.id }));
    } catch (err) {
      setError(err);
    }
  }, [version.id]);

  useEffect(() => {
    void load();
  }, [load]);

  const total = useMemo(() => lines.reduce((s, l) => s + l.planned_amount, 0), [lines]);

  return (
    <section aria-labelledby="lines-title" className="space-y-2">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h3 id="lines-title" className="text-sm font-semibold text-text-primary">
            {t("planning.lines", { title: version.title ?? `v${version.version_no}` })}
          </h3>
          <p className="text-xs text-text-muted">
            {canEdit
              ? t("planning.linesHint")
              : t("planning.linesLocked", { status: t(`versionStatus.${version.status}`) })}
          </p>
        </div>
        {canEdit ? (
          <Button
            size="sm"
            className="gap-1.5"
            onClick={() => setEditing("new")}
            disabled={centers.length === 0}
          >
            <Plus className="h-3.5 w-3.5" aria-hidden />
            {t("planning.newLine")}
          </Button>
        ) : null}
      </div>
      {error ? (
        <ErrorState error={error} title={t("planning.loadError")} onRetry={() => void load()} />
      ) : null}
      {lines.length === 0 ? (
        <EmptyState title={t("planning.noLines")} description={t("planning.noLinesBody")} />
      ) : (
        <div className="overflow-x-auto rounded-lg border border-surface-border">
          <table className="w-full text-sm">
            <thead className="bg-surface-2 text-xs uppercase tracking-wide text-text-secondary">
              <tr>
                <th scope="col" className={TH}>
                  {t("planning.col.costCenter")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.category")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.period")}
                </th>
                <th scope="col" className="px-3 py-2 text-end font-semibold">
                  {t("planning.col.amount")}
                </th>
                <th scope="col" className={TH}>
                  {t("planning.col.justification")}
                </th>
                <th scope="col" className="px-3 py-2">
                  <span className="sr-only">{t("planning.col.actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => (
                <tr key={l.id} className="border-t border-surface-border">
                  <td className="px-3 py-2">{l.cost_center_code}</td>
                  <td className="px-3 py-2">{categoryLabel(l.budget_bucket)}</td>
                  <td className="px-3 py-2">
                    {l.period_month ? periodLabel(l.period_month) : t("planning.annual")}
                  </td>
                  <td className="px-3 py-2 text-end font-mono tabular-nums">
                    {formatCurrency(l.planned_amount, version.currency_code)}
                  </td>
                  <td className="max-w-xs truncate px-3 py-2 text-text-secondary">
                    {l.justification_note ?? "—"}
                  </td>
                  <td className="px-3 py-2 text-end">
                    {canEdit ? (
                      <Button
                        size="sm"
                        variant="ghost"
                        className="h-7 w-7 p-0"
                        aria-label={t("planning.editLine")}
                        onClick={() => setEditing(l)}
                      >
                        <Pencil className="h-3.5 w-3.5" aria-hidden />
                      </Button>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr className="border-t border-surface-border font-semibold">
                <td className="px-3 py-2" colSpan={3}>
                  {t("planning.total")}
                </td>
                <td className="px-3 py-2 text-end font-mono tabular-nums">
                  {formatCurrency(total, version.currency_code)}
                </td>
                <td colSpan={2} />
              </tr>
            </tfoot>
          </table>
        </div>
      )}
      {editing ? (
        <LineDialog
          version={version}
          line={editing === "new" ? null : editing}
          centers={centers}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await load();
            await onChanged();
          }}
        />
      ) : null}
    </section>
  );
}

function LineDialog({
  version,
  line,
  centers,
  onClose,
  onSaved,
}: {
  version: BudgetVersion;
  line: BudgetLine | null;
  centers: CostCenter[];
  onClose: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation("budget");
  const { categories, categoryLabel, periodLabel } = useFinanceCatalogs();
  const [cc, setCc] = useState(line ? String(line.cost_center_id) : String(centers[0]?.id ?? ""));
  const [bucket, setBucket] = useState(line?.budget_bucket ?? "");
  // A new line starts on the first category of the reference catalog once it is loaded.
  useEffect(() => {
    if (!bucket && categories[0]) setBucket(categories[0].code);
  }, [bucket, categories]);
  // A line kept on a category since retired in the reference data still shows it.
  const options =
    bucket && !categories.some((c) => c.code === bucket)
      ? [...categories.map((c) => c.code), bucket]
      : categories.map((c) => c.code);
  const [month, setMonth] = useState(line?.period_month ? String(line.period_month) : ANNUAL);
  const [amount, setAmount] = useState(line ? String(line.planned_amount) : "");
  const [note, setNote] = useState(line?.justification_note ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const parsed = parseAmount(amount);
  const valid = parsed != null && parsed > 0 && cc !== "" && bucket !== "";

  const submit = async () => {
    if (!valid || saving || parsed == null) return;
    setSaving(true);
    setError(null);
    const period_month = month === ANNUAL ? null : Number(month);
    try {
      if (line) {
        await updateBudgetLine(line.id, line.row_version, {
          period_month,
          budget_bucket: bucket,
          planned_amount: parsed,
          justification_note: note.trim() || null,
        });
      } else {
        await createBudgetLine({
          budget_version_id: version.id,
          cost_center_id: Number(cc),
          period_month,
          budget_bucket: bucket,
          planned_amount: parsed,
          source_basis: "manual",
          justification_note: note.trim() || null,
        });
      }
      await onSaved();
    } catch (err) {
      setError(err);
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open onOpenChange={(open) => (open ? undefined : onClose())}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{line ? t("planning.editLine") : t("planning.newLine")}</DialogTitle>
          <DialogDescription>
            {t("planning.lineHint", { currency: version.currency_code })}
          </DialogDescription>
        </DialogHeader>
        <form
          className="space-y-3"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <div className="space-y-1">
            <Label htmlFor="line-cc">{t("planning.col.costCenter")}</Label>
            <Select value={cc} onValueChange={setCc} disabled={line != null}>
              <SelectTrigger id="line-cc">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {centers.map((c) => (
                  <SelectItem key={c.id} value={String(c.id)}>
                    {c.code} — {c.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="grid grid-cols-2 gap-3">
            <div className="space-y-1">
              <Label htmlFor="line-bucket">{t("planning.col.category")}</Label>
              <Select value={bucket} onValueChange={setBucket}>
                <SelectTrigger id="line-bucket">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {options.map((code) => (
                    <SelectItem key={code} value={code}>
                      {categoryLabel(code)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor="line-month">{t("planning.col.period")}</Label>
              <Select value={month} onValueChange={setMonth}>
                <SelectTrigger id="line-month">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={ANNUAL}>{t("planning.annual")}</SelectItem>
                  {Array.from({ length: 12 }, (_, i) => i + 1).map((m) => (
                    <SelectItem key={m} value={String(m)}>
                      {periodLabel(m)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="space-y-1">
            <Label htmlFor="line-amount">{t("planning.col.amount")}</Label>
            <Input
              id="line-amount"
              inputMode="decimal"
              value={amount}
              aria-invalid={!valid && amount !== ""}
              onChange={(e) => setAmount(e.target.value)}
            />
          </div>
          <div className="space-y-1">
            <Label htmlFor="line-note">{t("planning.col.justification")}</Label>
            <Textarea
              id="line-note"
              rows={2}
              value={note}
              onChange={(e) => setNote(e.target.value)}
            />
          </div>
          {error ? <ErrorState error={error} title={t("planning.saveError")} /> : null}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onClose}>
              {t("actions.cancel")}
            </Button>
            <Button type="submit" disabled={!valid || saving}>
              {saving ? t("actions.saving") : t("actions.save")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
