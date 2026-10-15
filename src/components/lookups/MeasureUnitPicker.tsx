/**
 * Units of measure from the governed reference list MEASURE.UNITS (backend `reference::units`).
 * Every unit picker of the app (articles, conversions, meters, inspection checkpoints) reads the
 * published list through the reference service and stores the unit's `reference_values.id`;
 * the frontend keeps no list of units of its own.
 *
 * A unit value carries its French label (`label`), and in `metadata_json` its English label
 * (`label_en`), its symbol (`symbol`), and for a plant unit the factor to its base standard unit
 * (`base_factor`, the base being `parent_id`). `origin: "system"` marks a protected standard
 * unit; `review_required` a unit created by migration 201 that an administrator must complete.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type { ReferenceValue } from "@shared/ipc-types";

export const MEASURE_UNITS_DOMAIN = "MEASURE.UNITS";
export const MEASURE_DIMENSIONS_DOMAIN = "MEASURE.DIMENSIONS";

export interface UnitMeta {
  origin: string | null;
  labelEn: string | null;
  symbol: string | null;
  baseFactor: number | null;
  reviewRequired: boolean;
  /** Every other key of `metadata_json`, kept when the editor writes the unit back. */
  rest: Record<string, unknown>;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() !== "" ? value.trim() : null;
}

/** Reads the unit fields of a `metadata_json` (tolerates a missing or invalid value). */
export function parseUnitMeta(metadataJson: string | null | undefined): UnitMeta {
  let raw: Record<string, unknown> = {};
  if (metadataJson) {
    try {
      const parsed: unknown = JSON.parse(metadataJson);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        raw = parsed as Record<string, unknown>;
      }
    } catch {
      raw = {};
    }
  }
  const factor = raw["base_factor"];
  const rest: Record<string, unknown> = { ...raw };
  for (const key of ["origin", "label_en", "symbol", "base_factor", "review_required"]) {
    delete rest[key];
  }
  return {
    origin: text(raw["origin"]),
    labelEn: text(raw["label_en"]),
    symbol: text(raw["symbol"]),
    baseFactor: typeof factor === "number" && Number.isFinite(factor) && factor > 0 ? factor : null,
    reviewRequired: raw["review_required"] === true,
    rest,
  };
}

/** `metadata_json` of a plant unit written by the editor. The review flag is dropped: the
 * administrator has just completed the unit (the server clears it too). */
export function buildPlantUnitMetadata(
  previous: string | null | undefined,
  fields: { symbol: string; labelEn: string; baseFactor: number },
): string {
  const meta = parseUnitMeta(previous);
  return JSON.stringify({
    ...meta.rest,
    ...(meta.origin ? { origin: meta.origin } : {}),
    label_en: fields.labelEn.trim(),
    symbol: fields.symbol.trim(),
    base_factor: fields.baseFactor,
  });
}

/** Protected standard unit shipped by Maintafox. */
export function isStandardUnit(value: ReferenceValue): boolean {
  return parseUnitMeta(value.metadata_json).origin === "system";
}

export function unitSymbol(value: ReferenceValue): string {
  return parseUnitMeta(value.metadata_json).symbol ?? value.code;
}

/** Label in the UI language (English label in English, French label otherwise). */
export function unitLabel(value: ReferenceValue, language: string): string {
  if (language.toLowerCase().startsWith("fr")) return value.label;
  return parseUnitMeta(value.metadata_json).labelEn ?? value.label;
}

/** `Kilogramme (kg)` — what a picker shows. */
export function unitOptionText(value: ReferenceValue, language: string): string {
  const symbol = unitSymbol(value);
  const label = unitLabel(value, language);
  return label === symbol ? label : `${label} (${symbol})`;
}

/** Positive finite number typed in a field (comma or dot), or null. */
export function parsePositiveNumber(raw: string): number | null {
  if (raw.trim() === "") return null;
  const n = Number(raw.replace(",", "."));
  return Number.isFinite(n) && n > 0 ? n : null;
}

function usePublishedValues(domainCode: string, enabled: boolean, reloadKey = 0) {
  const [values, setValues] = useState<ReferenceValue[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(enabled);
  useEffect(() => {
    if (!enabled) {
      setValues([]);
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    void listPublishedReferenceValuesByDomainCode(domainCode)
      .then((rows) => {
        if (!cancelled) setValues(rows);
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setValues([]);
          setError(err);
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [domainCode, enabled, reloadKey]);
  return { values, loading, error };
}

/** Active units of the published list; `reloadKey` reloads them (a "retry" button). */
export function useMeasureUnits(enabled = true, reloadKey = 0) {
  const { values, loading, error } = usePublishedValues(MEASURE_UNITS_DOMAIN, enabled, reloadKey);
  return { units: values, loading, error };
}

/** Physical dimensions (SYSTEM list), for the labels of the units editor. */
export function useMeasureDimensions(enabled = true) {
  const { values } = usePublishedValues(MEASURE_DIMENSIONS_DOMAIN, enabled);
  return values;
}

interface MeasureUnitSelectProps {
  id?: string;
  value: number | null;
  units: ReferenceValue[];
  onChange: (unitId: number | null) => void;
  /** Accessible name when no `<label htmlFor>` points at the select. */
  ariaLabel?: string;
  /** Offer "no unit" (meters and measurements may have none). */
  allowEmpty?: boolean;
  /** Text of the unit the record already has when it is no longer in the active list. */
  currentText?: string | null;
  disabled?: boolean;
  className?: string;
}

const SELECT_CLASS =
  "h-9 w-full rounded-md border border-surface-border bg-surface-0 px-2 text-sm text-text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60";

/** Native select of the governed units (keyboard and screen-reader friendly). */
export function MeasureUnitSelect({
  id,
  value,
  units,
  onChange,
  ariaLabel,
  allowEmpty = false,
  currentText,
  disabled,
  className,
}: MeasureUnitSelectProps) {
  const { t, i18n } = useTranslation("reference");
  const known = value == null || units.some((u) => u.id === value);
  return (
    <select
      id={id}
      value={value == null ? "" : String(value)}
      onChange={(e) => onChange(e.target.value === "" ? null : Number(e.target.value))}
      aria-label={ariaLabel}
      disabled={disabled}
      className={className ?? SELECT_CLASS}
    >
      {allowEmpty || value == null ? (
        <option value="">{allowEmpty ? t("units.none") : t("units.choose")}</option>
      ) : null}
      {units.map((u) => (
        <option key={u.id} value={String(u.id)}>
          {unitOptionText(u, i18n.language)}
        </option>
      ))}
      {known ? null : (
        <option value={String(value)}>
          {t("units.retired", { unit: currentText ?? String(value) })}
        </option>
      )}
    </select>
  );
}
