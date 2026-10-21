/**
 * Columns of the permit requirement rules (domain PERMIT.REQUIREMENT_RULES) in the Données de
 * référence editor. A rule says what it matches — an equipment class and / or a work type — and
 * the permit type it calls for; it is kept in the value's `metadata_json`
 * (`equipment_class_code`, `work_type_code`, `permit_type_code`). When a request becomes a work
 * order, a matching rule flags the work order "permit required" (backend
 * `permit::requirement_rules`, which also checks every rule when it is saved and published).
 *
 * The three choices come from their own lists: the published equipment classes and work types
 * (Données de référence) and the permit types (Permits, read with the permit view right).
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  ReferenceValueTableCell,
  ReferenceValueTableHeadCell,
} from "@/components/lookups/ReferenceValueTable";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { StatusChip } from "@/components/ui/status-chip";
import { mfInput } from "@/design-system/tokens";
import { useCanReadList } from "@/lib/list-access";
import { cn } from "@/lib/utils";
import { listPermitTypes } from "@/services/permit-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type { ReferenceValue } from "@shared/ipc-types";

export const PERMIT_RULES_DOMAIN = "PERMIT.REQUIREMENT_RULES";
const CLASS_DOMAIN = "EQUIPMENT.CLASS";
const WORK_TYPE_DOMAIN = "WORK.ORDER_TYPES";

export function isPermitRulesDomain(domainCode: string | null | undefined): boolean {
  return (domainCode ?? "").trim().toUpperCase() === PERMIT_RULES_DOMAIN;
}

/** The rule fields being edited ("" = not chosen). */
export interface PermitRuleEditState {
  classCode: string;
  workTypeCode: string;
  permitTypeCode: string;
}

export const EMPTY_PERMIT_RULE_EDIT: PermitRuleEditState = {
  classCode: "",
  workTypeCode: "",
  permitTypeCode: "",
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

function text(o: Record<string, unknown>, key: string): string {
  const v = o[key];
  return typeof v === "string" ? v.trim() : "";
}

export function permitRuleStateFrom(value: ReferenceValue): PermitRuleEditState {
  const o = metaObject(value.metadata_json);
  return {
    classCode: text(o, "equipment_class_code"),
    workTypeCode: text(o, "work_type_code"),
    permitTypeCode: text(o, "permit_type_code"),
  };
}

/** A rule names a permit type and at least an equipment class or a work type. */
export function permitRuleComplete(state: PermitRuleEditState): boolean {
  return state.permitTypeCode !== "" && (state.classCode !== "" || state.workTypeCode !== "");
}

/** `metadata_json` of a rule; other keys already on the value are kept. */
export function buildPermitRuleMetadata(
  existing: string | null,
  state: PermitRuleEditState,
): string {
  const o = metaObject(existing);
  delete o["equipment_class_code"];
  delete o["work_type_code"];
  if (state.classCode) o["equipment_class_code"] = state.classCode;
  if (state.workTypeCode) o["work_type_code"] = state.workTypeCode;
  o["permit_type_code"] = state.permitTypeCode;
  return JSON.stringify(o);
}

interface Option {
  code: string;
  label: string;
}

export interface PermitRuleOptions {
  classes: Option[];
  workTypes: Option[];
  permitTypes: Option[];
  /** The permit types may not be read without the permit view right. */
  canReadPermitTypes: boolean;
  /** True once the three lists have been read (problems are only judged then). */
  loaded: boolean;
  loading: boolean;
  error: unknown;
  reload: () => void;
}

/** The three lists a rule picks from (read only when the rules domain is open). */
export function usePermitRuleOptions(enabled: boolean): PermitRuleOptions {
  const canReadPermitTypes = useCanReadList("permitTypes");
  const [classes, setClasses] = useState<Option[]>([]);
  const [workTypes, setWorkTypes] = useState<Option[]>([]);
  const [permitTypes, setPermitTypes] = useState<Option[]>([]);
  const [loading, setLoading] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [round, setRound] = useState(0);
  const reload = useCallback(() => setRound((n) => n + 1), []);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    const toOptions = (rows: ReferenceValue[]) =>
      rows.filter((r) => r.is_active).map((r) => ({ code: r.code, label: r.label }));
    void Promise.all([
      listPublishedReferenceValuesByDomainCode(CLASS_DOMAIN),
      listPublishedReferenceValuesByDomainCode(WORK_TYPE_DOMAIN),
      canReadPermitTypes ? listPermitTypes() : Promise.resolve([]),
    ])
      .then(([cls, wts, pts]) => {
        if (cancelled) return;
        setClasses(toOptions(cls));
        setWorkTypes(toOptions(wts));
        setPermitTypes(pts.map((p) => ({ code: p.code, label: p.name })));
        setLoaded(true);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [enabled, canReadPermitTypes, round]);

  return { classes, workTypes, permitTypes, canReadPermitTypes, loaded, loading, error, reload };
}

/** Why the conversion cannot use a rule, as the backend judges it; null when it can. */
export type PermitRuleProblem =
  | { kind: "incomplete" }
  | { kind: "unknownClass"; code: string }
  | { kind: "unknownWorkType"; code: string }
  | { kind: "unknownPermitType"; code: string };

export function permitRuleProblem(
  state: PermitRuleEditState,
  options: PermitRuleOptions,
): PermitRuleProblem | null {
  if (!permitRuleComplete(state)) return { kind: "incomplete" };
  if (!options.loaded) return null;
  const has = (list: Option[], code: string) => list.some((o) => o.code === code);
  if (state.classCode && !has(options.classes, state.classCode)) {
    return { kind: "unknownClass", code: state.classCode };
  }
  if (state.workTypeCode && !has(options.workTypes, state.workTypeCode)) {
    return { kind: "unknownWorkType", code: state.workTypeCode };
  }
  if (options.canReadPermitTypes && !has(options.permitTypes, state.permitTypeCode)) {
    return { kind: "unknownPermitType", code: state.permitTypeCode };
  }
  return null;
}

/** Active rules of the list that the conversion cannot use. */
export function unusablePermitRules(
  values: ReferenceValue[],
  options: PermitRuleOptions,
): ReferenceValue[] {
  return values.filter(
    (v) => v.is_active && permitRuleProblem(permitRuleStateFrom(v), options) != null,
  );
}

function optionText(options: Option[], code: string): string {
  if (!code) return "—";
  const found = options.find((o) => o.code === code);
  return found ? `${found.code} — ${found.label}` : code;
}

export function PermitRuleHeadCells() {
  const { t } = useTranslation("reference");
  return (
    <>
      <ReferenceValueTableHeadCell>{t("permitRules.colClass")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("permitRules.colWorkType")}</ReferenceValueTableHeadCell>
      <ReferenceValueTableHeadCell>{t("permitRules.colPermitType")}</ReferenceValueTableHeadCell>
    </>
  );
}

/** Read-only cells of a rule; a rule that does not name what it needs is marked. */
export function PermitRuleReadCells({
  value,
  options,
}: {
  value: ReferenceValue;
  options: PermitRuleOptions;
}) {
  const { t } = useTranslation("reference");
  const state = permitRuleStateFrom(value);
  const problem = permitRuleProblem(state, options);
  return (
    <>
      <ReferenceValueTableCell className="text-xs">
        {optionText(options.classes, state.classCode)}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="text-xs">
        {optionText(options.workTypes, state.workTypeCode)}
      </ReferenceValueTableCell>
      <ReferenceValueTableCell className="space-y-1 text-xs">
        {problem?.kind === "incomplete" ? (
          <StatusChip
            label={t("permitRules.incomplete")}
            tone="danger"
            title={t("permitRules.incompleteHint")}
          />
        ) : (
          <>
            <span className="block">{optionText(options.permitTypes, state.permitTypeCode)}</span>
            {problem ? (
              <>
                <StatusChip label={t("permitRules.unusable")} tone="danger" />
                <span className="block text-tone-danger-fg">
                  {t(`permitRules.problem.${problem.kind}`, { code: problem.code })}
                </span>
              </>
            ) : null}
          </>
        )}
      </ReferenceValueTableCell>
    </>
  );
}

function RuleSelect({
  value,
  options,
  onChange,
  label,
  noneLabel,
}: {
  value: string;
  options: Option[];
  onChange: (code: string) => void;
  label: string;
  noneLabel: string;
}) {
  // A code already on the rule but no longer in the list stays visible as written.
  const unknown = value !== "" && !options.some((o) => o.code === value);
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      aria-label={label}
      className={cn(mfInput.nativeSelect, "h-9")}
    >
      <option value="">{noneLabel}</option>
      {options.map((o) => (
        <option key={o.code} value={o.code}>
          {o.code} — {o.label}
        </option>
      ))}
      {unknown ? <option value={value}>{value}</option> : null}
    </select>
  );
}

/** Editable cells of a rule (new row or row being edited). */
export function PermitRuleEditCells({
  state,
  onChange,
  options,
}: {
  state: PermitRuleEditState;
  onChange: (next: PermitRuleEditState) => void;
  options: PermitRuleOptions;
}) {
  const { t } = useTranslation("reference");
  return (
    <>
      <ReferenceValueTableCell>
        <RuleSelect
          value={state.classCode}
          options={options.classes}
          onChange={(classCode) => onChange({ ...state, classCode })}
          label={t("permitRules.colClass")}
          noneLabel={t("permitRules.anyClass")}
        />
      </ReferenceValueTableCell>
      <ReferenceValueTableCell>
        <RuleSelect
          value={state.workTypeCode}
          options={options.workTypes}
          onChange={(workTypeCode) => onChange({ ...state, workTypeCode })}
          label={t("permitRules.colWorkType")}
          noneLabel={t("permitRules.anyWorkType")}
        />
      </ReferenceValueTableCell>
      <ReferenceValueTableCell>
        {options.canReadPermitTypes ? (
          <RuleSelect
            value={state.permitTypeCode}
            options={options.permitTypes}
            onChange={(permitTypeCode) => onChange({ ...state, permitTypeCode })}
            label={t("permitRules.colPermitType")}
            noneLabel={t("permitRules.choosePermitType")}
          />
        ) : (
          <ListAccessNote list="permitTypes" />
        )}
        {!permitRuleComplete(state) ? (
          <p className="mt-1 text-xs text-tone-warning-fg">{t("permitRules.incompleteHint")}</p>
        ) : null}
      </ReferenceValueTableCell>
    </>
  );
}
