/**
 * Where a block (RBD) or basic event (fault tree) takes its value from: the study fit, an
 * equipment's recorded MTBF, a Weibull or exponential life, a reliability, a probability or a
 * failure rate typed by the analyst. Shared by the system model editors.
 */
import { useTranslation } from "react-i18next";

import { DecimalField } from "@/components/ui/decimal-field";
import { useFormatters } from "@/hooks/use-formatters";
import type { EquipmentLife, LifeSource } from "@/services/reliability-model-service";

import { RAMS_SELECT_CLASS } from "./rams-ui";

export type SourceKind = LifeSource["kind"];

export function defaultSource(kind: SourceKind, equipment: EquipmentLife[]): LifeSource | null {
  switch (kind) {
    case "study_fit":
      return { kind };
    case "equipment_history": {
      const first = equipment.find((e) => e.mtbf_hours != null) ?? equipment[0];
      return first ? { kind, equipment_id: first.equipment_id } : null;
    }
    // Typed values start empty (NaN) so nothing is assumed: the analyst must enter them.
    case "weibull":
      return { kind, beta: Number.NaN, eta: Number.NaN };
    case "exponential":
      return { kind, mtbf_hours: Number.NaN };
    case "reliability":
    case "probability":
      return { kind, value: Number.NaN };
    case "rate":
      return { kind, per_hour: Number.NaN };
  }
}

function NumberField({
  label,
  value,
  onChange,
  decimals = 4,
}: {
  label: string;
  /** NaN means "not entered yet": the field is empty. */
  value: number;
  onChange: (v: number) => void;
  decimals?: number;
}) {
  return (
    <label className="grid gap-1 text-xs font-medium text-text-secondary">
      {label}
      <DecimalField
        className="h-10"
        decimals={decimals}
        value={Number.isFinite(value) ? value : null}
        onValueChange={(v) => onChange(v ?? Number.NaN)}
      />
    </label>
  );
}

export function SourceFields({
  source,
  kinds,
  equipment,
  fit,
  disabled,
  onChange,
}: {
  source: LifeSource | null;
  kinds: SourceKind[];
  equipment: EquipmentLife[];
  fit: [number, number] | null;
  disabled?: boolean;
  onChange: (s: LifeSource | null) => void;
}) {
  const { t } = useTranslation("reliability");
  const { formatDecimal, formatNumber } = useFormatters();
  return (
    <div className="grid gap-2">
      <label className="grid gap-1 text-xs font-medium text-text-secondary">
        {t("rams.system.source")}
        <select
          className={RAMS_SELECT_CLASS}
          value={source?.kind ?? ""}
          disabled={disabled}
          onChange={(e) => onChange(defaultSource(e.target.value as SourceKind, equipment))}
        >
          <option value="" disabled>
            {t("rams.fmeca.choose")}
          </option>
          {kinds.map((k) => (
            <option key={k} value={k} disabled={k === "study_fit" && fit == null}>
              {t(`rams.system.sourceKind.${k}`)}
              {k === "study_fit" && fit == null ? ` (${t("rams.system.noFit")})` : ""}
            </option>
          ))}
        </select>
      </label>
      {source?.kind === "study_fit" && fit ? (
        <p className="text-xs text-text-muted">
          {t("rams.system.fitValues", {
            beta: formatDecimal(fit[0], 2),
            eta: formatNumber(Math.round(fit[1])),
          })}
        </p>
      ) : null}
      {source?.kind === "equipment_history" ? (
        <label className="grid gap-1 text-xs font-medium text-text-secondary">
          {t("rams.system.equipment")}
          <select
            className={RAMS_SELECT_CLASS}
            value={source.equipment_id}
            disabled={disabled}
            onChange={(e) =>
              onChange({ kind: "equipment_history", equipment_id: Number(e.target.value) })
            }
          >
            {equipment.map((eq) => (
              <option key={eq.equipment_id} value={eq.equipment_id}>
                {eq.mtbf_hours != null
                  ? t("rams.system.equipmentOption", {
                      code: eq.asset_code,
                      mtbf: formatNumber(Math.round(eq.mtbf_hours)),
                      failures: eq.failures,
                    })
                  : t("rams.system.equipmentNoFailure", { code: eq.asset_code })}
              </option>
            ))}
          </select>
        </label>
      ) : null}
      {source?.kind === "weibull" ? (
        <div className="grid grid-cols-2 gap-2">
          <NumberField
            label={t("rams.system.beta")}
            value={source.beta}
            onChange={(v) => onChange({ ...source, beta: v })}
          />
          <NumberField
            label={t("rams.system.eta")}
            value={source.eta}
            onChange={(v) => onChange({ ...source, eta: v })}
          />
        </div>
      ) : null}
      {source?.kind === "exponential" ? (
        <NumberField
          label={t("rams.system.mtbf")}
          value={source.mtbf_hours}
          onChange={(v) => onChange({ ...source, mtbf_hours: v })}
        />
      ) : null}
      {source?.kind === "reliability" ? (
        <NumberField
          label={t("rams.system.reliabilityValue")}
          decimals={6}
          value={source.value}
          onChange={(v) => onChange({ ...source, value: v })}
        />
      ) : null}
      {source?.kind === "probability" ? (
        <NumberField
          label={t("rams.system.probabilityValue")}
          decimals={6}
          value={source.value}
          onChange={(v) => onChange({ ...source, value: v })}
        />
      ) : null}
      {source?.kind === "rate" ? (
        <NumberField
          label={t("rams.system.rate")}
          decimals={8}
          value={source.per_hour}
          onChange={(v) => onChange({ ...source, per_hour: v })}
        />
      ) : null}
    </div>
  );
}

/** Source sent to the backend: a source whose typed values are not all filled in is sent as
 * "no source", which the evaluation reports on that block or event. */
export function serializeSource(s: LifeSource | null): LifeSource | null {
  if (!s) return null;
  const numbersOk = Object.values(s).every((v) => typeof v !== "number" || Number.isFinite(v));
  return numbersOk ? s : null;
}
