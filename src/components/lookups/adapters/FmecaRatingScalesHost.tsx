/**
 * Échelles AMDEC — the plant's FMECA rating grids (severity, occurrence, detection, criticality
 * zones), kept in Données de référence (SYSTEM lists `RELIABILITY.FMECA_*`: scores and zone codes
 * fixed by Maintafox). The plant edits the definitions (FR / EN), the RCM consequence class, the
 * occurrence thresholds and whether a detection level is hidden. Who may edit is unchanged:
 * the reliability manage right (`ram.manage`). Every FMECA study of the plant reads these grids.
 */
import { Pencil } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { RatingScalesDialog } from "@/components/reliability/study/RatingScalesDialog";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type RatingScales,
  getReliabilityRatingScales,
} from "@/services/reliability-model-service";
import { P } from "@shared/rbac/permissions.generated";

const KINDS = ["severity", "occurrence", "detection"] as const;

export function FmecaRatingScalesHost() {
  const { t, i18n } = useTranslation("reliability");
  const { can } = usePermissions();
  const canEdit = can(P.RAM_MANAGE);
  const [scales, setScales] = useState<RatingScales | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);
  const [open, setOpen] = useState(false);
  const fr = (i18n.resolvedLanguage ?? i18n.language ?? "fr").toLowerCase().startsWith("fr");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setScales(await getReliabilityRatingScales());
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (error != null)
    return (
      <ErrorState
        error={error}
        title={t("rams.scales.loadError")}
        onRetry={() => void load()}
        retrying={loading}
      />
    );
  if (scales == null) return <LoadingState />;

  return (
    <div className="space-y-4 p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1 space-y-1">
          <h2 className="text-base font-semibold text-text-primary">{t("rams.scales.title")}</h2>
          <p className="text-sm text-text-secondary">{t("rams.scales.hostHint")}</p>
          <p className="text-xs text-text-muted">
            {canEdit ? t("rams.scales.whoEdits") : t("rams.scales.readOnly")}
          </p>
        </div>
        <Button variant="outline" className="h-10" onClick={() => setOpen(true)}>
          <Pencil className="h-4 w-4" aria-hidden />
          {canEdit ? t("rams.scales.edit") : t("rams.scales.view")}
        </Button>
      </div>
      {scales.review ? (
        <p
          role="alert"
          className="rounded-md border border-tone-attention-border bg-tone-attention-bg px-3 py-2 text-sm text-tone-attention-fg"
        >
          {t("rams.scales.reviewFormerUnreadable")}
        </p>
      ) : null}
      <div className="grid gap-3 md:grid-cols-3">
        {KINDS.map((k) => (
          <section key={k} className="rounded-md border border-surface-border p-3">
            <h3 className="mb-2 text-sm font-medium text-text-primary">{t(`rams.fmeca.${k}`)}</h3>
            <ol className="space-y-1 text-xs text-text-secondary">
              {[...scales[k]]
                .sort((a, b) => b.score - a.score)
                .map((l) => (
                  <li key={l.score} className="flex gap-2">
                    <span className="w-5 shrink-0 font-mono text-text-primary">{l.score}</span>
                    <span>{fr ? l.label_fr : l.label_en}</span>
                  </li>
                ))}
            </ol>
          </section>
        ))}
      </div>
      <RatingScalesDialog
        open={open}
        onOpenChange={setOpen}
        scales={scales}
        editable={canEdit}
        onSaved={() => void load()}
      />
    </div>
  );
}
