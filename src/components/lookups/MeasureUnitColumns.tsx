/**
 * Units-of-measure columns of the Données de référence editor (domain MEASURE.UNITS).
 *
 * A standard unit (kg, m, h, bar, °C…) is shipped by Maintafox and protected: shown with its
 * symbol, dimension and UN/CEFACT Rec 20 code, never edited. A plant unit names its base standard
 * unit and the factor to it ("1 SAC = 25 kg"); both are required and checked on the server, which
 * also gives the plant unit the base unit's dimension. A unit created by migration 201 for a code
 * or a text Maintafox did not know is flagged "to review" until an administrator completes it.
 */

import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import {
  isStandardUnit,
  parsePositiveNumber,
  parseUnitMeta,
  unitLabel,
  unitSymbol,
} from "@/components/lookups/MeasureUnitPicker";
import {
  ReferenceValueTableCell,
  ReferenceValueTableHeadCell,
} from "@/components/lookups/ReferenceValueTable";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { mfInput } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import type { ReferenceValue } from "@shared/ipc-types";

/** Unit fields being edited (strings as typed). */
export interface UnitEditState {
  symbol: string;
  labelEn: string;
  baseId: number | null;
  factor: string;
}

export const EMPTY_UNIT_EDIT: UnitEditState = { symbol: "", labelEn: "", baseId: null, factor: "" };

export function unitEditStateFrom(value: ReferenceValue): UnitEditState {
  const meta = parseUnitMeta(value.metadata_json);
  return {
    symbol: meta.symbol ?? "",
    labelEn: meta.labelEn ?? "",
    baseId: value.parent_id,
    factor: meta.baseFactor != null ? String(meta.baseFactor) : "",
  };
}

/** A plant unit can be saved once it has a symbol, a base unit and a positive factor. */
export function unitEditComplete(state: UnitEditState): boolean {
  return (
    state.symbol.trim() !== "" && state.baseId != null && parsePositiveNumber(state.factor) != null
  );
}

export function isUnitToReview(value: ReferenceValue): boolean {
  return parseUnitMeta(value.metadata_json).reviewRequired;
}

/** Active standard units of the set: the possible bases of a plant unit. */
export function baseUnitCandidates(values: ReferenceValue[]): ReferenceValue[] {
  return values.filter((v) => v.is_active && isStandardUnit(v) && v.semantic_tag != null);
}

export function UnitHeadCells() {
  const { t } = useTranslation("reference");
  return (
    <>
      <ReferenceValueTableHeadCell>{t("units.colSymbol")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("units.colLabelEn")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("units.colDimension")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("units.colBase")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("units.colRec20")}</ReferenceValueTableHeadCell>
    </>
  );
}

function dimensionText(
  tag: string | null,
  dimensions: ReferenceValue[],
  language: string,
): string | null {
  if (!tag) return null;
  const dim = dimensions.find((d) => d.code === tag);
  return dim ? unitLabel(dim, language) : tag;
}

/** Read-only cells of a unit row. */
export function UnitReadCells({
  value,
  values,
  dimensions,
}: {
  value: ReferenceValue;
  values: ReferenceValue[];
  dimensions: ReferenceValue[];
}) {
  const { t, i18n } = useTranslation("reference");
  const meta = parseUnitMeta(value.metadata_json);
  const base = value.parent_id != null ? values.find((v) => v.id === value.parent_id) : undefined;
  const dimension = dimensionText(value.semantic_tag, dimensions, i18n.language);
  return (
    <>
      <ReferenceValueTableCell>
        <span className="font-mono text-xs">{unitSymbol(value)}</span>
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs text-text-secondary">
        {meta.labelEn ?? "—"}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs text-text-secondary">
        {dimension ?? t("units.noDimension")}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs">
        {isStandardUnit(value) ? (
          <span className="text-text-muted">{t("units.standard")}</span>
        ) : base && meta.baseFactor != null ? (
          <span className="font-mono">
            {t("units.factorRule", {
              unit: unitSymbol(value),
              factor: meta.baseFactor,
              base: unitSymbol(base),
            })}
          </span>
        ) : (
          <StatusChip
            label={t("units.toReview")}
            tone="attention"
            title={t("units.toReviewHint")}
          />
        )}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="font-mono text-xs text-text-muted">
        {value.external_code ?? "—"}
      </ReferenceValueTableCell>
    </>
  );
}

/** Editable cells of a plant unit (new row or row being edited). */
export function UnitEditCells({
  state,
  onChange,
  candidates,
  dimensions,
  onKeyDown,
}: {
  state: UnitEditState;
  onChange: (next: UnitEditState) => void;
  candidates: ReferenceValue[];
  dimensions: ReferenceValue[];
  onKeyDown?: (e: KeyboardEvent) => void;
}) {
  const { t, i18n } = useTranslation("reference");
  const base = candidates.find((c) => c.id === state.baseId);
  const factorInvalid = state.factor.trim() !== "" && parsePositiveNumber(state.factor) == null;
  return (
    <>
      <ReferenceValueTableCell>
        <Input
          value={state.symbol}
          onChange={(e) => onChange({ ...state, symbol: e.target.value })}
          onKeyDown={onKeyDown}
          aria-label={t("units.colSymbol")}
          placeholder={t("units.symbolPlaceholder")}
          className="h-9 w-24 text-sm"
        />
      </ReferenceValueTableCell>
      <ReferenceValueTableCell>
        <Input
          value={state.labelEn}
          onChange={(e) => onChange({ ...state, labelEn: e.target.value })}
          onKeyDown={onKeyDown}
          aria-label={t("units.colLabelEn")}
          className="h-9 text-sm"
        />
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs text-text-secondary">
        {base
          ? dimensionText(base.semantic_tag, dimensions, i18n.language)
          : t("units.dimensionFromBase")}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell>
        <div className="flex items-center gap-1.5">
          <Input
            value={state.factor}
            onChange={(e) => onChange({ ...state, factor: e.target.value })}
            onKeyDown={onKeyDown}
            inputMode="decimal"
            aria-label={t("units.factor")}
            aria-invalid={factorInvalid}
            placeholder={t("units.factor")}
            className="h-9 w-20 text-sm"
          />
          <select
            value={state.baseId == null ? "" : String(state.baseId)}
            onChange={(e) =>
              onChange({ ...state, baseId: e.target.value ? Number(e.target.value) : null })
            }
            aria-label={t("units.baseUnit")}
            className={cn(mfInput.nativeSelect, "h-9 w-auto")}
          >
            <option value="">{t("units.chooseBase")}</option>
            {candidates.map((c) => (
              <option key={c.id} value={String(c.id)}>
                {`${unitLabel(c, i18n.language)} (${unitSymbol(c)})`}
              </option>
            ))}
          </select>
        </div>
        {factorInvalid ? (
          <p className="mt-1 text-xs text-text-danger">{t("units.factorInvalid")}</p>
        ) : null}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs text-text-muted">—</ReferenceValueTableCell>
    </>
  );
}
