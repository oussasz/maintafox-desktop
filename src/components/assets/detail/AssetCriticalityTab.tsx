/**
 * Criticality tab: rate the asset with the documented risk-matrix method (consequence × likelihood,
 * safety / environment override), see the resulting class before saving, and the history of
 * assessments. The rule and its thresholds come from the backend (`assets::criticality`).
 */

import { ShieldAlert } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { CriticalityBadge } from "@/components/assets/CriticalityBadge";
import { CriticalityReviewMark } from "@/components/assets/CriticalityReviewMark";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Textarea } from "@/components/ui/textarea";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { cn } from "@/lib/utils";
import {
  assessAssetCriticality,
  getAssetCriticalityMethod,
  listAssetCriticalityAssessments,
  previewAssetCriticality,
} from "@/services/asset-registry-service";
import type {
  Asset,
  CriticalityAssessment,
  CriticalityMethod,
  CriticalityResult,
  CriticalityScores,
} from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const CATEGORIES = ["safety", "environment", "production", "quality", "cost"] as const;
/** Backend limit (`criticality::RATIONALE_MAX_CHARS`). */
const RATIONALE_MAX_CHARS = 2000;

interface CriticalityData {
  method: CriticalityMethod;
  history: CriticalityAssessment[];
}

function scoresFrom(a: CriticalityAssessment | undefined, fallback: number): CriticalityScores {
  return {
    safety: a?.safety_score ?? fallback,
    environment: a?.environment_score ?? fallback,
    production: a?.production_score ?? fallback,
    quality: a?.quality_score ?? fallback,
    cost: a?.cost_score ?? fallback,
    likelihood: a?.likelihood_score ?? fallback,
  };
}

function ScoreRow({
  label,
  hint,
  value,
  method,
  disabled,
  onChange,
}: {
  label: string;
  hint: string;
  value: number;
  method: CriticalityMethod;
  disabled: boolean;
  onChange: (v: number) => void;
}) {
  const options: number[] = [];
  for (let v = method.score_min; v <= method.score_max; v += 1) options.push(v);
  return (
    <div className="grid gap-1 sm:grid-cols-[12rem_1fr] sm:items-center">
      <div>
        <p className="text-sm font-medium">{label}</p>
        <p className="text-[11px] text-text-muted">{hint}</p>
      </div>
      <div className="flex gap-1" role="radiogroup" aria-label={label}>
        {options.map((v) => (
          <button
            key={v}
            type="button"
            role="radio"
            aria-checked={value === v}
            disabled={disabled}
            onClick={() => onChange(v)}
            className={cn(
              "h-10 w-10 rounded-md border text-sm font-medium tabular-nums focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-60",
              value === v
                ? "border-primary bg-primary text-primary-foreground"
                : "border-surface-border bg-surface-1 hover:bg-surface-2",
            )}
          >
            {v}
          </button>
        ))}
      </div>
    </div>
  );
}

export function AssetCriticalityTab({
  asset,
  onAssetChanged,
}: {
  asset: Asset;
  onAssetChanged: (asset: Asset) => void;
}) {
  const { t } = useTranslation("equipment");
  const { formatDateTime } = useFormatters();
  const { can } = usePermissions();
  const canAssess = can(P.EQ_MANAGE);
  const load = useMemo(
    () => async (): Promise<CriticalityData> => {
      const [method, history] = await Promise.all([
        getAssetCriticalityMethod(),
        listAssetCriticalityAssessments(asset.id),
      ]);
      return { method, history };
    },
    [asset.id],
  );
  const { data, error, loading, reload } = useLoad(load, `${asset.id}:${asset.row_version}`);
  const [scores, setScores] = useState<CriticalityScores | null>(null);
  const [rationale, setRationale] = useState("");
  const [preview, setPreview] = useState<CriticalityResult | null>(null);
  const [saving, setSaving] = useState(false);
  const debouncedScores = useDebouncedValue(scores, 200);

  // Start from the latest assessment, or the lowest score of the scale.
  useEffect(() => {
    if (data) setScores(scoresFrom(data.history[0], data.method.score_min));
  }, [data]);

  useEffect(() => {
    if (!debouncedScores) return;
    let cancelled = false;
    previewAssetCriticality(debouncedScores)
      .then((r) => {
        if (!cancelled) setPreview(r);
      })
      .catch(() => {
        // Reported by the global IPC toast.
        if (!cancelled) setPreview(null);
      });
    return () => {
      cancelled = true;
    };
  }, [debouncedScores]);

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  if (!scores) return <LoadingState />;
  const { method, history } = data;

  const setScore = (key: keyof CriticalityScores, v: number) =>
    setScores((s) => (s ? { ...s, [key]: v } : s));

  const save = async () => {
    setSaving(true);
    try {
      const result = await assessAssetCriticality(
        asset.id,
        scores,
        rationale.trim() || null,
        asset.row_version,
      );
      setRationale("");
      onAssetChanged(result.asset);
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-4">
      <DetailSectionCard title={t("critMethod.title")} icon={ShieldAlert}>
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm text-text-muted">{t("critMethod.current")}</span>
          <CriticalityBadge criticality={asset.criticality_code} />
          {asset.criticality_to_review ? <CriticalityReviewMark /> : null}
        </div>
        {asset.criticality_to_review ? (
          <p className="text-xs text-tone-warning-fg">{t("criticalityReview.hint")}</p>
        ) : null}
        <p className="text-xs text-text-muted">
          {t("critMethod.explain", {
            a: method.class_a_min_risk,
            b: method.class_b_min_risk,
            c: method.class_c_min_risk,
            hse: method.hse_override_score,
            min: method.score_min,
            max: method.score_max,
          })}
        </p>
        <div className="space-y-3 pt-2">
          {CATEGORIES.map((c) => (
            <ScoreRow
              key={c}
              label={t(`critMethod.categories.${c}`)}
              hint={t(`critMethod.hints.${c}`)}
              value={scores[c]}
              method={method}
              disabled={!canAssess || saving}
              onChange={(v) => setScore(c, v)}
            />
          ))}
          <ScoreRow
            label={t("critMethod.categories.likelihood")}
            hint={t("critMethod.hints.likelihood")}
            value={scores.likelihood}
            method={method}
            disabled={!canAssess || saving}
            onChange={(v) => setScore("likelihood", v)}
          />
        </div>
        <div
          className="mt-3 flex flex-wrap items-center gap-3 rounded-md border border-surface-border bg-surface-1 p-3"
          aria-live="polite"
        >
          {preview ? (
            <>
              <span className="text-sm">
                {t("critMethod.preview", {
                  consequence: preview.consequence,
                  likelihood: scores.likelihood,
                  risk: preview.risk_score,
                })}
              </span>
              <CriticalityBadge criticality={preview.resulting_class} />
              {preview.hse_override ? (
                <span className="text-xs text-text-muted">{t("critMethod.hseOverride")}</span>
              ) : null}
            </>
          ) : (
            <span className="text-sm text-text-muted">{t("critMethod.previewPending")}</span>
          )}
        </div>
        {canAssess ? (
          <div className="space-y-2 pt-2">
            <label className="block text-sm font-medium" htmlFor={`crit-rationale-${asset.id}`}>
              {t("critMethod.rationale")}
            </label>
            <Textarea
              id={`crit-rationale-${asset.id}`}
              rows={3}
              value={rationale}
              maxLength={RATIONALE_MAX_CHARS}
              placeholder={t("critMethod.rationalePlaceholder")}
              onChange={(e) => setRationale(e.target.value)}
            />
            <div className="flex justify-end">
              <Button className="h-11" disabled={saving || !preview} onClick={() => void save()}>
                {saving ? t("commercial.saving") : t("critMethod.save")}
              </Button>
            </div>
          </div>
        ) : (
          <p className="text-xs text-text-muted">{t("critMethod.noPermission")}</p>
        )}
      </DetailSectionCard>

      <DetailSectionCard title={t("critMethod.history")} icon={ShieldAlert}>
        {history.length === 0 ? (
          <p className="text-sm text-text-muted">{t("critMethod.noHistory")}</p>
        ) : (
          <ul className="divide-y divide-surface-border">
            {history.map((a) => (
              <li key={a.id} className="space-y-1 py-2">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <CriticalityBadge criticality={a.resulting_class} compact />
                  <span>
                    {t("critMethod.historyLine", {
                      risk: a.risk_score,
                      previous: a.previous_class ?? "—",
                    })}
                  </span>
                  <span className="text-xs text-text-muted">
                    {formatDateTime(a.assessed_at)}
                    {a.assessed_by_name ? ` · ${a.assessed_by_name}` : ""}
                  </span>
                </div>
                <p className="text-xs text-text-muted">
                  {t("critMethod.historyScores", {
                    s: a.safety_score,
                    e: a.environment_score,
                    p: a.production_score,
                    q: a.quality_score,
                    c: a.cost_score,
                    l: a.likelihood_score,
                  })}
                </p>
                {a.rationale ? <p className="text-xs">{a.rationale}</p> : null}
              </li>
            ))}
          </ul>
        )}
      </DetailSectionCard>
    </div>
  );
}
