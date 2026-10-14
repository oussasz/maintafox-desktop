/**
 * Plant FMECA rating grids: severity (with its RCM consequence class), occurrence (failures per
 * equipment-year), detection (hidden or evident). Read for everyone, edited with `ram.manage`.
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useFormatters } from "@/hooks/use-formatters";
import {
  type RatingScales,
  saveReliabilityRatingScales,
} from "@/services/reliability-model-service";

import { RAMS_SELECT_CLASS, RAMS_TABLE_FRAME } from "./rams-ui";

type Kind = "severity" | "occurrence" | "detection";

export function RatingScalesDialog({
  open,
  onOpenChange,
  scales,
  editable,
  onSaved,
}: {
  open: boolean;
  onOpenChange: (o: boolean) => void;
  scales: RatingScales;
  editable: boolean;
  onSaved: () => void;
}) {
  const { t, i18n } = useTranslation("reliability");
  const { formatDecimal } = useFormatters();
  const [draft, setDraft] = useState<RatingScales>(scales);
  const [kind, setKind] = useState<Kind>("severity");
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (open) setDraft(scales);
  }, [open, scales]);
  const fr = (i18n.resolvedLanguage ?? i18n.language ?? "fr").toLowerCase().startsWith("fr");
  const labelKey = fr ? "label_fr" : "label_en";

  const update = (score: number, patch: Partial<RatingScales["severity"][number]>) =>
    setDraft((d) => ({
      ...d,
      [kind]: d[kind].map((l) => (l.score === score ? { ...l, ...patch } : l)),
    }));

  const levels = [...draft[kind]].sort((a, b) => b.score - a.score);
  return (
    <Dialog open={open} onOpenChange={(o) => !saving && onOpenChange(o)}>
      <DialogContent className="max-w-3xl">
        <DialogHeader>
          <DialogTitle>{t("rams.scales.title")}</DialogTitle>
          <DialogDescription>{t("rams.scales.hint")}</DialogDescription>
        </DialogHeader>
        <div
          role="tablist"
          aria-label={t("rams.scales.title")}
          className="flex gap-1 border-b border-surface-border"
        >
          {(["severity", "occurrence", "detection"] as Kind[]).map((k) => (
            <button
              key={k}
              type="button"
              role="tab"
              aria-selected={kind === k}
              onClick={() => setKind(k)}
              className={`min-h-[44px] border-b-2 px-3 text-sm ${
                kind === k
                  ? "border-primary font-semibold text-primary"
                  : "border-transparent text-text-secondary"
              }`}
            >
              {t(`rams.fmeca.${k}`)}
            </button>
          ))}
        </div>
        <div className={`${RAMS_TABLE_FRAME} max-h-[50vh] overflow-y-auto`}>
          <table className="w-full text-sm">
            <thead>
              <tr className="bg-surface-2 text-[11px] uppercase tracking-wide text-text-secondary">
                <th className="px-2 py-2 text-start">{t("rams.scales.score")}</th>
                <th className="px-2 py-2 text-start">{t("rams.scales.definition")}</th>
                <th className="px-2 py-2 text-start">{t(`rams.scales.extra.${kind}`)}</th>
              </tr>
            </thead>
            <tbody>
              {levels.map((l) => (
                <tr key={l.score} className="border-t border-surface-border">
                  <td className="px-2 py-1.5 font-mono">{l.score}</td>
                  <td className="px-2 py-1.5">
                    {editable ? (
                      <Input
                        className="h-9"
                        aria-label={t("rams.scales.definitionOf", { score: l.score })}
                        value={l[labelKey]}
                        onChange={(e) => update(l.score, { [labelKey]: e.target.value })}
                      />
                    ) : (
                      l[labelKey]
                    )}
                  </td>
                  <td className="px-2 py-1.5">
                    {kind === "severity" ? (
                      editable ? (
                        <select
                          className={RAMS_SELECT_CLASS}
                          aria-label={t("rams.scales.consequenceOf", { score: l.score })}
                          value={l.consequence ?? ""}
                          onChange={(e) => update(l.score, { consequence: e.target.value })}
                        >
                          {["safety", "operational", "non_operational"].map((c) => (
                            <option key={c} value={c}>
                              {t(`rams.scales.consequence.${c}`)}
                            </option>
                          ))}
                        </select>
                      ) : (
                        t(`rams.scales.consequence.${l.consequence ?? "non_operational"}`)
                      )
                    ) : kind === "occurrence" ? (
                      editable ? (
                        <Input
                          className="h-9 w-28"
                          inputMode="decimal"
                          aria-label={t("rams.scales.thresholdOf", { score: l.score })}
                          value={String(l.min_per_year ?? 0)}
                          onChange={(e) => {
                            const v = Number(e.target.value.replace(",", "."));
                            if (Number.isFinite(v)) update(l.score, { min_per_year: v });
                          }}
                        />
                      ) : (
                        t("rams.scales.perYear", { value: formatDecimal(l.min_per_year ?? 0, 2) })
                      )
                    ) : editable ? (
                      <label className="inline-flex min-h-9 items-center gap-2">
                        <input
                          type="checkbox"
                          className="h-4 w-4 accent-primary"
                          checked={l.hidden ?? false}
                          onChange={(e) => update(l.score, { hidden: e.target.checked })}
                        />
                        {t("rams.scales.hidden")}
                      </label>
                    ) : l.hidden ? (
                      t("rams.scales.hidden")
                    ) : (
                      t("rams.scales.evident")
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <DialogFooter className="gap-2">
          <Button
            variant="outline"
            className="h-11"
            onClick={() => onOpenChange(false)}
            disabled={saving}
          >
            {editable ? t("rams.common.cancel") : t("rams.common.close")}
          </Button>
          {editable ? (
            <Button
              className="h-11"
              disabled={saving}
              onClick={async () => {
                setSaving(true);
                try {
                  await saveReliabilityRatingScales(draft);
                  onOpenChange(false);
                  onSaved();
                } catch {
                  // global IPC toast (validation codes)
                } finally {
                  setSaving(false);
                }
              }}
            >
              {t("rams.scales.save")}
            </Button>
          ) : null}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
