/**
 * Validation list (L7-32): skills recorded without validation (an import brings them in that
 * way) and certificates still waiting for verification. A `trn.certify` holder filters the list,
 * ticks the lines checked against the evidence and validates them at once. The server validates
 * each line exactly as shown here and audits it; a line changed since the list was read is
 * refused and stays in the list. Verifying certificates asks for the step-up, as on the sheet.
 */
import { BadgeCheck } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useStepUp } from "@/hooks/use-step-up";
import {
  type PendingQualification,
  type QualificationValidationResult,
  listPendingQualifications,
  validatePendingQualifications,
} from "@/services/qualification-service";
import { isStepUpRequiredError } from "@/utils/errors";
import { P } from "@shared/rbac/permissions.generated";

const ALL = "__all__";
type KindFilter = typeof ALL | PendingQualification["kind"];

function lineKey(l: { kind: string; id: number }): string {
  return `${l.kind}:${l.id}`;
}

/** What the line says: a tick holds only while the line still says it (re-checked on reload). */
function lineContent(l: PendingQualification): string {
  return [l.proficiency_level ?? "", l.valid_to ?? "", l.row_version ?? ""].join("|");
}

function isCancelled(err: unknown): boolean {
  return err instanceof Error && err.message === "Step-up cancelled by user";
}

export interface PendingValidationPanelProps {
  refreshKey?: number;
  /** Called after lines were validated (the matrix and expiries change). */
  onChanged?: () => void;
}

export function PendingValidationPanel({ refreshKey = 0, onChanged }: PendingValidationPanelProps) {
  const { t } = useTranslation("training");
  const { formatDate } = useFormatters();
  const { can } = usePermissions();
  const { withStepUp, StepUpDialogElement } = useStepUp();
  const canCertify = can(P.TRN_CERTIFY);
  const [lines, setLines] = useState<PendingQualification[] | null>(null);
  const [validatorLinked, setValidatorLinked] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [kind, setKind] = useState<KindFilter>(ALL);
  const [item, setItem] = useState<string>(ALL);
  const [team, setTeam] = useState<string>(ALL);
  const [importedOnly, setImportedOnly] = useState(false);
  const [search, setSearch] = useState("");
  // Ticked lines, with the content they had when ticked.
  const [selected, setSelected] = useState<Map<string, string>>(new Map());
  const [confirming, setConfirming] = useState(false);
  const [refused, setRefused] = useState<Map<string, string>>(new Map());
  const [lastValidated, setLastValidated] = useState<number | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await listPendingQualifications();
      setLines(next.lines);
      setValidatorLinked(next.validator_linked);
      // A ticked line no longer waiting (validated elsewhere) or changed since it was ticked
      // leaves the selection: it is checked against its evidence again before validating.
      const now = new Map(next.lines.map((l) => [lineKey(l), lineContent(l)]));
      setSelected((s) => new Map([...s].filter(([k, c]) => now.get(k) === c)));
    } catch (err) {
      setError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const itemOptions = useMemo(() => {
    const m = new Map<string, string>();
    for (const l of lines ?? [])
      if (kind === ALL || l.kind === kind) m.set(l.item_code, l.item_label);
    return [...m.entries()].sort((a, b) => a[1].localeCompare(b[1]));
  }, [lines, kind]);

  const teamOptions = useMemo(
    () =>
      [...new Set((lines ?? []).map((l) => l.team_name).filter((x): x is string => !!x))].sort(),
    [lines],
  );

  const visible = useMemo(() => {
    const q = search.trim().toLowerCase();
    return (lines ?? []).filter(
      (l) =>
        (kind === ALL || l.kind === kind) &&
        (item === ALL || l.item_code === item) &&
        (team === ALL || l.team_name === team) &&
        (!importedOnly || l.source_type === "import") &&
        (q === "" ||
          l.full_name.toLowerCase().includes(q) ||
          l.employee_code.toLowerCase().includes(q)),
    );
  }, [lines, kind, item, team, importedOnly, search]);

  // Validating a skill also records it on the personnel sheet (as on the sheet itself) and names
  // the validator's personnel record: an account without one validates certificates only.
  const canValidateSkills = canCertify && can(P.PER_MANAGE) && validatorLinked;
  const ticked = (l: PendingQualification) => selected.get(lineKey(l)) === lineContent(l);

  const selectable = useCallback(
    (l: PendingQualification) => canCertify && (l.kind !== "skill" || canValidateSkills),
    [canCertify, canValidateSkills],
  );

  const visibleSelectable = visible.filter(selectable);
  const allVisibleTicked = visibleSelectable.length > 0 && visibleSelectable.every(ticked);
  // Only ticked lines still in view are validated: a filter change never validates hidden lines.
  const chosen = visibleSelectable.filter(ticked);
  const chosenSkills = chosen.filter((l) => l.kind === "skill").length;
  const chosenCerts = chosen.length - chosenSkills;

  const toggle = (l: PendingQualification, on: boolean) =>
    setSelected((s) => {
      const n = new Map(s);
      if (on) n.set(lineKey(l), lineContent(l));
      else n.delete(lineKey(l));
      return n;
    });

  const toggleAll = (on: boolean) =>
    setSelected((s) => {
      const n = new Map(s);
      for (const l of visibleSelectable) {
        if (on) n.set(lineKey(l), lineContent(l));
        else n.delete(lineKey(l));
      }
      return n;
    });

  const validate = async () => {
    let results: QualificationValidationResult[];
    try {
      results =
        chosenCerts > 0
          ? await withStepUp(() => validatePendingQualifications(chosen))
          : await validatePendingQualifications(chosen);
    } catch (err) {
      // A cancelled step-up closes quietly; other failures are reported by the global toast.
      if (isCancelled(err) || isStepUpRequiredError(err)) return;
      // Show the list as it now stands before the failure is reported.
      await load();
      throw err;
    }
    const nextRefused = new Map<string, string>();
    for (const r of results) if (!r.validated) nextRefused.set(lineKey(r), r.code ?? "");
    setRefused(nextRefused);
    setLastValidated(results.filter((r) => r.validated).length);
    await load();
    onChanged?.();
  };

  const refusalText = (code: string) =>
    t(`errors.${code}`, { defaultValue: t("validation.refusedUnknown") });

  if (error && !lines)
    return <ErrorState error={error} onRetry={() => void load()} retrying={loading} />;
  if (loading && !lines) return <LoadingState />;

  return (
    <div className="space-y-4">
      <p className="text-sm text-text-muted">{t("validation.intro")}</p>
      <div className="flex flex-wrap items-center gap-3">
        <Input
          aria-label={t("validation.search")}
          placeholder={t("validation.search")}
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="w-56"
        />
        <Select
          value={kind}
          onValueChange={(v) => {
            setKind(v as KindFilter);
            setItem(ALL);
          }}
        >
          <SelectTrigger aria-label={t("validation.kind")} className="w-48">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>{t("validation.allKinds")}</SelectItem>
            <SelectItem value="skill">{t("validation.skills")}</SelectItem>
            <SelectItem value="certification">{t("validation.certifications")}</SelectItem>
          </SelectContent>
        </Select>
        <Select value={item} onValueChange={setItem}>
          <SelectTrigger aria-label={t("validation.item")} className="w-56">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>{t("validation.allItems")}</SelectItem>
            {itemOptions.map(([code, label]) => (
              <SelectItem key={code} value={code}>
                {label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select value={team} onValueChange={setTeam}>
          <SelectTrigger aria-label={t("filter.team")} className="w-48">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL}>{t("filter.allTeams")}</SelectItem>
            {teamOptions.map((name) => (
              <SelectItem key={name} value={name}>
                {name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <label className="flex items-center gap-2 text-sm text-text-secondary">
          <Checkbox checked={importedOnly} onCheckedChange={setImportedOnly} />
          {t("validation.importedOnly")}
        </label>
      </div>

      {lastValidated != null ? (
        <p role="status" className="text-sm text-text-secondary">
          {t("validation.done", { count: lastValidated })}
          {refused.size > 0 ? ` ${t("validation.someRefused", { count: refused.size })}` : null}
        </p>
      ) : null}

      {visible.length === 0 ? (
        <EmptyState
          icon={<BadgeCheck className="h-8 w-8" aria-hidden />}
          title={t("validation.empty")}
          description={t("validation.emptyHint")}
        />
      ) : (
        <div className="rounded-md border border-surface-border">
          <div className="flex flex-wrap items-center justify-between gap-2 border-b border-surface-border bg-surface-1 px-3 py-2">
            {canCertify ? (
              <label className="flex items-center gap-2 text-sm">
                <Checkbox
                  checked={allVisibleTicked}
                  onCheckedChange={toggleAll}
                  disabled={visibleSelectable.length === 0}
                />
                {t("validation.selectAll", { count: visibleSelectable.length })}
              </label>
            ) : (
              <span className="text-sm text-text-muted">{t("validation.readOnly")}</span>
            )}
            {canCertify && can(P.PER_MANAGE) && !validatorLinked ? (
              <span className="text-xs text-text-muted">{t("validation.skillsNeedLink")}</span>
            ) : null}
            {canCertify ? (
              <Button size="sm" disabled={chosen.length === 0} onClick={() => setConfirming(true)}>
                {t("validation.validate", { count: chosen.length })}
              </Button>
            ) : null}
          </div>
          <ul className="divide-y divide-surface-border">
            {visible.map((l) => {
              const key = lineKey(l);
              const refusal = refused.get(key);
              return (
                <li key={key} className="flex flex-wrap items-center gap-3 px-3 py-2 text-sm">
                  {canCertify ? (
                    <Checkbox
                      aria-label={t("validation.selectLine", {
                        name: l.full_name,
                        item: l.item_label,
                      })}
                      checked={ticked(l)}
                      onCheckedChange={(on) => toggle(l, on)}
                      disabled={!selectable(l)}
                    />
                  ) : null}
                  <span className="min-w-[200px] flex-1">
                    <span className="font-medium text-text-primary">{l.full_name}</span>
                    <span className="ml-2 text-xs text-text-muted">
                      {[l.employee_code, l.team_name].filter(Boolean).join(" · ")}
                    </span>
                  </span>
                  <span className="min-w-[180px] text-text-primary">
                    {l.item_label}
                    <span className="ml-2 text-xs text-text-muted">
                      {l.kind === "skill" ? t("validation.skill") : t("validation.certification")}
                    </span>
                  </span>
                  <span className="min-w-[160px] text-text-secondary">
                    {l.kind === "skill"
                      ? t(`level.${l.proficiency_level ?? ""}`, {
                          defaultValue: String(l.proficiency_level ?? "—"),
                        })
                      : [l.certificate_ref, l.issuing_body].filter(Boolean).join(" · ") || "—"}
                  </span>
                  <span className="tabular-nums text-text-secondary">
                    {l.valid_to
                      ? t("validation.until", { date: formatDate(l.valid_to) })
                      : t("validation.noEnd")}
                  </span>
                  {l.source_type === "import" ? (
                    <span className="text-xs text-text-muted">{t("validation.imported")}</span>
                  ) : null}
                  {refusal != null ? (
                    <span role="alert" className="basis-full text-xs text-status-danger">
                      {refusalText(refusal)}
                    </span>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </div>
      )}

      <ConfirmDialog
        open={confirming}
        onOpenChange={setConfirming}
        title={t("validation.confirmTitle")}
        description={t("validation.confirmBody", {
          skills: chosenSkills,
          certifications: chosenCerts,
        })}
        confirmLabel={t("validation.validate", { count: chosen.length })}
        onConfirm={validate}
      />
      {StepUpDialogElement}
    </div>
  );
}
