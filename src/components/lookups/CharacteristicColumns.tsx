/**
 * Columns of the technical characteristics per family (domain EQUIPMENT.CHARACTERISTICS) in the
 * Données de référence editor. Each value is one characteristic of one family (its parent): the
 * label is the field's label, and `metadata_json` says what it holds — `data_type` (number, text,
 * yes / no), `unit_code` (a unit of MEASURE.UNITS, numbers only) and `required`. The asset record
 * shows these fields for every equipment of the family (backend `assets::characteristics`, which
 * also checks the definition when it is saved).
 */
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  ReferenceValueTableCell,
  ReferenceValueTableHeadCell,
} from "@/components/lookups/ReferenceValueTable";
import { mfInput } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type { CharacteristicDataType, ReferenceValue } from "@shared/ipc-types";

export const CHARACTERISTICS_DOMAIN = "EQUIPMENT.CHARACTERISTICS";
const UNITS_DOMAIN = "MEASURE.UNITS";
const DATA_TYPES: CharacteristicDataType[] = ["number", "text", "boolean"];

export function isCharacteristicsDomain(domainCode: string | null | undefined): boolean {
  return (domainCode ?? "").trim().toUpperCase() === CHARACTERISTICS_DOMAIN;
}

/** The definition being edited ("" = not chosen). */
export interface CharacteristicEditState {
  dataType: CharacteristicDataType | "";
  unitCode: string;
  required: boolean;
}

export const EMPTY_CHARACTERISTIC_EDIT: CharacteristicEditState = {
  dataType: "",
  unitCode: "",
  required: false,
};

function metaObject(metadataJson: string | null): Record<string, unknown> {
  if (!metadataJson) return {};
  try {
    const parsed = JSON.parse(metadataJson) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

export function characteristicStateFrom(value: ReferenceValue): CharacteristicEditState {
  const o = metaObject(value.metadata_json);
  const dt = typeof o["data_type"] === "string" ? o["data_type"] : "";
  return {
    dataType: (DATA_TYPES as string[]).includes(dt) ? (dt as CharacteristicDataType) : "",
    unitCode: typeof o["unit_code"] === "string" ? o["unit_code"] : "",
    required: o["required"] === true,
  };
}

/** A definition names its value type; only a number has a unit. */
export function characteristicComplete(state: CharacteristicEditState): boolean {
  return state.dataType !== "" && (state.dataType === "number" || state.unitCode === "");
}

/** `metadata_json` of a characteristic; other keys already on the value are kept. */
export function buildCharacteristicMetadata(
  existing: string | null,
  state: CharacteristicEditState,
): string {
  const o = metaObject(existing);
  o["data_type"] = state.dataType;
  o["unit_code"] = state.dataType === "number" && state.unitCode ? state.unitCode : null;
  o["required"] = state.required;
  return JSON.stringify(o);
}

interface UnitOption {
  code: string;
  label: string;
}

/** The published units (read only while the characteristics list is open). */
export function useCharacteristicUnits(enabled: boolean): {
  units: UnitOption[];
  error: unknown;
} {
  const [units, setUnits] = useState<UnitOption[]>([]);
  const [error, setError] = useState<unknown>(null);
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    listPublishedReferenceValuesByDomainCode(UNITS_DOMAIN)
      .then((rows) => {
        if (!cancelled) {
          setUnits(rows.filter((r) => r.is_active).map((r) => ({ code: r.code, label: r.label })));
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled]);
  return { units, error };
}

export function CharacteristicHeadCells() {
  const { t } = useTranslation("reference");
  return (
    <>
      <ReferenceValueTableHeadCell>{t("characteristics.colType")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("characteristics.colUnit")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("characteristics.colRequired")}</ReferenceValueTableHeadCell>
    </>
  );
}

export function CharacteristicReadCells({ value }: { value: ReferenceValue }) {
  const { t } = useTranslation("reference");
  const state = characteristicStateFrom(value);
  return (
    <>
      <ReferenceValueTableCell className="text-xs">
        {state.dataType ? (
          t(`characteristics.types.${state.dataType}`)
        ) : (
          <span className="text-tone-danger-fg">{t("characteristics.typeMissing")}</span>
        )}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs">{state.unitCode || "—"}</ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs">
        {state.required ? t("characteristics.yes") : t("characteristics.no")}
      </ReferenceValueTableCell>
    </>
  );
}

export function CharacteristicEditCells({
  state,
  onChange,
  units,
}: {
  state: CharacteristicEditState;
  onChange: (next: CharacteristicEditState) => void;
  units: UnitOption[];
}) {
  const { t } = useTranslation("reference");
  const unknownUnit = state.unitCode !== "" && !units.some((u) => u.code === state.unitCode);
  return (
    <>
      <ReferenceValueTableCell>
        <select
          value={state.dataType}
          aria-label={t("characteristics.colType")}
          className={cn(mfInput.nativeSelect, "h-9")}
          onChange={(e) => {
            const dataType = e.target.value as CharacteristicDataType | "";
            onChange({ ...state, dataType, unitCode: dataType === "number" ? state.unitCode : "" });
          }}
        >
          <option value="">{t("characteristics.chooseType")}</option>
          {DATA_TYPES.map((d) => (
            <option key={d} value={d}>
              {t(`characteristics.types.${d}`)}
            </option>
          ))}
        </select>
      </ReferenceValueTableCell>
      <ReferenceValueTableCell>
        <select
          value={state.unitCode}
          aria-label={t("characteristics.colUnit")}
          className={cn(mfInput.nativeSelect, "h-9")}
          disabled={state.dataType !== "number"}
          onChange={(e) => onChange({ ...state, unitCode: e.target.value })}
        >
          <option value="">{t("characteristics.noUnit")}</option>
          {units.map((u) => (
            <option key={u.code} value={u.code}>
              {u.code} — {u.label}
            </option>
          ))}
          {unknownUnit ? <option value={state.unitCode}>{state.unitCode}</option> : null}
        </select>
      </ReferenceValueTableCell>
      <ReferenceValueTableCell>
        <label className="inline-flex items-center gap-1.5 text-xs">
          <input
            type="checkbox"
            checked={state.required}
            onChange={(e) => onChange({ ...state, required: e.target.checked })}
          />
          {t("characteristics.colRequired")}
        </label>
      </ReferenceValueTableCell>
    </>
  );
}
