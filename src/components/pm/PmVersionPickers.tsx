/**
 * Version form building blocks: tolerance, usage counter (meter of the asset or meter type),
 * structured required parts (inventory articles + quantity) and required skills.
 */
import { Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import { type InventoryArticleRow, listInventoryArticles } from "@/services/inventory-service";
import { getLookupValues } from "@/services/lookup-service";
import { type PmAssetMeter, listPmAssetMeters } from "@/services/pm-service";
import { listPublishedReferenceValuesByDomainCode } from "@/services/reference-service";
import type { LookupValueOption, ReferenceValue } from "@shared/ipc-types";

import type { PmMeterSpec, PmRequiredPart, PmTolerance } from "./pm-model";

/** Domain of required skills (validated by the backend on save). */
const SKILLS_DOMAIN = "PERSONNEL.SKILLS";
/** Lookup domain of meter types (HOURS, CYCLES, DISTANCE, …). */
const METER_TYPE_DOMAIN = "equipment.meter_type";
/** Articles shown per search in the parts picker. */
const ARTICLE_SEARCH_LIMIT = 8;

function optionalNumber(raw: string): number | null {
  if (!raw.trim()) return null;
  const n = Number(raw.replace(",", "."));
  return Number.isFinite(n) ? n : null;
}

// ── Tolerance ────────────────────────────────────────────────────────────────

export function PmToleranceFields({
  value,
  onChange,
  percentAllowed,
}: {
  value: PmTolerance | null;
  onChange: (next: PmTolerance | null) => void;
  /** A percentage needs an interval (calendar or meter). */
  percentAllowed: boolean;
}) {
  const { t } = useTranslation("pm");
  const unit = value?.unit ?? "none";
  return (
    <fieldset className="grid gap-2 rounded-md border p-3 md:col-span-2">
      <legend className="px-1 text-sm font-medium">{t("tolerance.title")}</legend>
      <p className="text-xs text-text-muted">{t("tolerance.hint")}</p>
      <div className="grid gap-2 sm:grid-cols-3">
        <div className="grid gap-1">
          <Label htmlFor="pm-tol-unit">{t("tolerance.unit")}</Label>
          <Select
            value={unit}
            onValueChange={(v) =>
              onChange(
                v === "none"
                  ? null
                  : {
                      unit: v as PmTolerance["unit"],
                      early: value?.early ?? 0,
                      late: value?.late ?? 0,
                    },
              )
            }
          >
            <SelectTrigger id="pm-tol-unit">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="none">{t("tolerance.none")}</SelectItem>
              <SelectItem value="days">{t("tolerance.days")}</SelectItem>
              {percentAllowed ? (
                <SelectItem value="percent">{t("tolerance.percent")}</SelectItem>
              ) : null}
            </SelectContent>
          </Select>
        </div>
        {value ? (
          <>
            <div className="grid gap-1">
              <Label htmlFor="pm-tol-early">{t("tolerance.early")}</Label>
              <Input
                id="pm-tol-early"
                type="number"
                min={0}
                value={value.early}
                onChange={(e) =>
                  onChange({ ...value, early: Math.max(0, optionalNumber(e.target.value) ?? 0) })
                }
              />
            </div>
            <div className="grid gap-1">
              <Label htmlFor="pm-tol-late">{t("tolerance.late")}</Label>
              <Input
                id="pm-tol-late"
                type="number"
                min={0}
                value={value.late}
                onChange={(e) =>
                  onChange({ ...value, late: Math.max(0, optionalNumber(e.target.value) ?? 0) })
                }
              />
            </div>
          </>
        ) : null}
      </div>
    </fieldset>
  );
}

// ── Usage counter ────────────────────────────────────────────────────────────

export function PmMeterSpecFields({
  idPrefix,
  value,
  onChange,
  equipmentId,
}: {
  idPrefix: string;
  value: PmMeterSpec;
  onChange: (next: PmMeterSpec) => void;
  /** The plan's single asset: its meters are offered; otherwise a meter type is chosen. */
  equipmentId: number | null;
}) {
  const { t, i18n } = useTranslation("pm");
  const { formatNumber } = useFormatters();
  const [meters, setMeters] = useState<PmAssetMeter[] | null>(null);
  const [types, setTypes] = useState<LookupValueOption[]>([]);
  /** A failed load is said next to the field: "no meter" would be a false statement. */
  const [metersFailed, setMetersFailed] = useState(false);
  const [typesFailed, setTypesFailed] = useState(false);
  const [attempt, setAttempt] = useState(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: attempt = explicit retry
  useEffect(() => {
    let cancelled = false;
    setMetersFailed(false);
    setTypesFailed(false);
    if (equipmentId != null) {
      listPmAssetMeters(equipmentId)
        .then((rows) => {
          if (!cancelled) setMeters(rows);
        })
        .catch(() => {
          if (!cancelled) {
            setMeters(null);
            setMetersFailed(true);
          }
        });
    } else {
      setMeters(null);
    }
    getLookupValues(METER_TYPE_DOMAIN)
      .then((rows) => {
        if (!cancelled) setTypes(rows.filter((r) => r.is_active === 1));
      })
      .catch(() => {
        if (!cancelled) setTypesFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [equipmentId, attempt]);

  const loadFailedNotice =
    metersFailed || typesFailed ? (
      <p
        role="alert"
        className="flex flex-wrap items-center gap-2 text-xs text-tone-danger-fg md:col-span-2"
      >
        {t("errors.listLoad")}
        <Button
          type="button"
          variant="outline"
          className="h-8 px-2 text-xs"
          onClick={() => setAttempt((n) => n + 1)}
        >
          {t("actions.retry")}
        </Button>
      </p>
    ) : null;

  const selected = meters?.find((m) => m.id === value.assetMeterId) ?? null;
  const unit = selected?.unit?.trim() || null;
  const typeLabel = (row: LookupValueOption) =>
    (i18n.language.startsWith("fr") ? row.fr_label : row.en_label) || row.label;

  return (
    <div className="grid gap-2 md:col-span-2 md:grid-cols-2">
      {equipmentId != null ? (
        metersFailed ? (
          loadFailedNotice
        ) : meters != null && meters.length === 0 ? (
          <p className="text-sm text-tone-warning-fg md:col-span-2">{t("hints.noMeters")}</p>
        ) : (
          <div className="grid gap-1 md:col-span-2">
            <Label htmlFor={`${idPrefix}-meter`}>{t("fields.meter")}</Label>
            <Select
              value={value.assetMeterId != null ? String(value.assetMeterId) : ""}
              onValueChange={(id) => {
                const meter = meters?.find((m) => String(m.id) === id) ?? null;
                onChange({
                  ...value,
                  assetMeterId: meter?.id ?? null,
                  meterType: null,
                  startReading: value.startReading ?? meter?.lifetime_reading ?? null,
                });
              }}
            >
              <SelectTrigger id={`${idPrefix}-meter`}>
                <SelectValue placeholder={t("errors.meterRequired")} />
              </SelectTrigger>
              <SelectContent>
                {(meters ?? []).map((m) => (
                  <SelectItem key={m.id} value={String(m.id)}>
                    {m.name}
                    {m.unit ? ` (${m.unit})` : ""}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {selected ? (
              <p className="text-xs text-text-muted">
                {t("fields.meterLifetime", {
                  value: formatNumber(selected.lifetime_reading),
                  unit: unit ?? "",
                })}
              </p>
            ) : null}
          </div>
        )
      ) : (
        <div className="grid gap-1 md:col-span-2">
          <Label htmlFor={`${idPrefix}-type`}>{t("fields.meterType")}</Label>
          <Select
            value={value.meterType ?? ""}
            onValueChange={(code) => onChange({ ...value, meterType: code, assetMeterId: null })}
          >
            <SelectTrigger id={`${idPrefix}-type`}>
              <SelectValue placeholder={t("errors.meterTypeRequired")} />
            </SelectTrigger>
            <SelectContent>
              {types.map((row) => (
                <SelectItem key={row.id} value={row.code}>
                  {typeLabel(row)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <p className="text-xs text-text-muted">{t("hints.meterTypePerAsset")}</p>
          {typesFailed ? loadFailedNotice : null}
        </div>
      )}
      <div className="grid gap-1">
        <Label htmlFor={`${idPrefix}-interval`}>
          {unit ? t("fields.meterInterval", { unit }) : t("fields.meterIntervalNoUnit")}
        </Label>
        <Input
          id={`${idPrefix}-interval`}
          type="number"
          min={1}
          value={value.interval}
          onChange={(e) =>
            onChange({ ...value, interval: Math.max(1, optionalNumber(e.target.value) ?? 1) })
          }
        />
      </div>
      <div className="grid gap-1">
        <Label htmlFor={`${idPrefix}-start`}>
          {unit ? t("fields.startReading", { unit }) : t("fields.startReadingNoUnit")}
        </Label>
        <Input
          id={`${idPrefix}-start`}
          type="number"
          min={0}
          value={value.startReading ?? ""}
          onChange={(e) => onChange({ ...value, startReading: optionalNumber(e.target.value) })}
        />
        <p className="text-xs text-text-muted">{t("hints.startReadingOptional")}</p>
      </div>
    </div>
  );
}

// ── Required parts ───────────────────────────────────────────────────────────

export function PmPartsPicker({
  value,
  onChange,
}: {
  value: PmRequiredPart[];
  onChange: (next: PmRequiredPart[]) => void;
}) {
  const { t } = useTranslation("pm");
  const { formatNumber } = useFormatters();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<InventoryArticleRow[]>([]);
  /** The search failed (not "no article matches"). */
  const [searchFailed, setSearchFailed] = useState(false);
  const [names, setNames] = useState<Record<string, InventoryArticleRow>>({});

  useEffect(() => {
    const q = query.trim();
    setSearchFailed(false);
    if (q.length < 2) {
      setResults([]);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      listInventoryArticles({ search: q })
        .then((rows) => {
          if (!cancelled)
            setResults(rows.filter((r) => r.is_active === 1).slice(0, ARTICLE_SEARCH_LIMIT));
        })
        .catch(() => {
          if (!cancelled) {
            setResults([]);
            setSearchFailed(true);
          }
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  // Names of parts already on the version (looked up once by code).
  useEffect(() => {
    const missing = value.map((p) => p.articleCode).filter((code) => !(code in names));
    if (missing.length === 0) return;
    let cancelled = false;
    Promise.all(missing.map((code) => listInventoryArticles({ search: code }).catch(() => [])))
      .then((lists) => {
        if (cancelled) return;
        const next: Record<string, InventoryArticleRow> = {};
        lists.flat().forEach((row) => {
          if (missing.includes(row.article_code)) next[row.article_code] = row;
        });
        setNames((prev) => ({ ...prev, ...next }));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [value, names]);

  const add = (row: InventoryArticleRow) => {
    setNames((prev) => ({ ...prev, [row.article_code]: row }));
    if (!value.some((p) => p.articleCode === row.article_code)) {
      onChange([...value, { articleCode: row.article_code, quantity: 1 }]);
    }
    setQuery("");
    setResults([]);
  };

  return (
    <fieldset className="grid gap-2 rounded-md border p-3 md:col-span-2">
      <legend className="px-1 text-sm font-medium">{t("fields.requiredParts")}</legend>
      {value.length === 0 ? (
        <p className="text-xs text-text-muted">{t("empty.noParts")}</p>
      ) : (
        <ul className="divide-y">
          {value.map((part, idx) => {
            const article = names[part.articleCode];
            return (
              <li
                key={part.articleCode}
                className="flex flex-wrap items-center gap-2 py-1.5 text-sm"
              >
                <span className="min-w-0 flex-1">
                  <span className="font-mono text-xs">{part.articleCode}</span>
                  {article ? <span className="ml-2">{article.article_name}</span> : null}
                  {article ? (
                    <span className="ml-2 text-xs text-text-muted">
                      {t("fields.available", {
                        value: formatNumber(article.available_qty),
                        unit: article.unit_label,
                      })}
                    </span>
                  ) : null}
                  {part.quantity == null ? (
                    <span className="block text-xs text-tone-warning-fg">
                      {t("hints.partNoQuantity")}
                    </span>
                  ) : null}
                </span>
                <Label className="sr-only" htmlFor={`pm-part-qty-${idx}`}>
                  {t("fields.quantity")}
                </Label>
                <Input
                  id={`pm-part-qty-${idx}`}
                  className="w-24"
                  type="number"
                  min={0}
                  value={part.quantity ?? ""}
                  aria-label={t("fields.quantityOf", { code: part.articleCode })}
                  onChange={(e) =>
                    onChange(
                      value.map((p, i) =>
                        i === idx ? { ...p, quantity: optionalNumber(e.target.value) } : p,
                      ),
                    )
                  }
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="h-9 w-9 p-0"
                  aria-label={t("actions.removePart", { code: part.articleCode })}
                  onClick={() => onChange(value.filter((_, i) => i !== idx))}
                >
                  <Trash2 className="h-4 w-4" aria-hidden />
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      <div className="grid gap-1">
        <Label htmlFor="pm-part-search">{t("fields.addPart")}</Label>
        <Input
          id="pm-part-search"
          value={query}
          placeholder={t("fields.articleSearchPlaceholder")}
          onChange={(e) => setQuery(e.target.value)}
        />
        {searchFailed ? (
          <p role="alert" className="text-xs text-tone-danger-fg">
            {t("errors.articleSearchFailed")}
          </p>
        ) : null}
        {results.length > 0 ? (
          <ul className="rounded-md border" role="listbox" aria-label={t("fields.addPart")}>
            {results.map((row) => (
              <li key={row.id}>
                <button
                  type="button"
                  role="option"
                  aria-selected={false}
                  className="flex w-full min-h-9 items-center justify-between gap-2 px-2 py-1 text-left text-sm hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  onClick={() => add(row)}
                >
                  <span>
                    <span className="font-mono text-xs">{row.article_code}</span>
                    <span className="ml-2">{row.article_name}</span>
                  </span>
                  <span className="text-xs text-text-muted">
                    {t("fields.available", {
                      value: formatNumber(row.available_qty),
                      unit: row.unit_label,
                    })}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
      <p className="text-xs text-text-muted">{t("hints.partsReserved")}</p>
    </fieldset>
  );
}

// ── Required skills ──────────────────────────────────────────────────────────

export function PmSkillsPicker({
  value,
  onChange,
}: {
  value: string[];
  onChange: (next: string[]) => void;
}) {
  const { t } = useTranslation("pm");
  const [skills, setSkills] = useState<ReferenceValue[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    listPublishedReferenceValuesByDomainCode(SKILLS_DOMAIN)
      .then((rows) => {
        if (!cancelled) setSkills(rows.filter((r) => r.is_active));
      })
      .catch(() => {
        // Reported by the global IPC toast.
        if (!cancelled) setSkills([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selected = useMemo(() => new Set(value), [value]);
  const unknown = value.filter((code) => skills != null && !skills.some((s) => s.code === code));

  return (
    <fieldset className="grid gap-2 rounded-md border p-3 md:col-span-2">
      <legend className="px-1 text-sm font-medium">{t("fields.requiredSkills")}</legend>
      {skills != null && skills.length === 0 ? (
        <p className="text-xs text-text-muted">{t("empty.noSkills")}</p>
      ) : (
        <div className="grid gap-1 sm:grid-cols-2">
          {(skills ?? []).map((skill) => (
            <label key={skill.id} className="flex min-h-9 items-center gap-2 text-sm">
              <Checkbox
                checked={selected.has(skill.code)}
                onCheckedChange={(checked) =>
                  onChange(
                    checked === true
                      ? [...value, skill.code]
                      : value.filter((code) => code !== skill.code),
                  )
                }
              />
              {skill.label}
            </label>
          ))}
        </div>
      )}
      {unknown.length > 0 ? (
        <p className="text-xs text-tone-warning-fg">
          {t("hints.unknownSkills", { codes: unknown.join(", ") })}
        </p>
      ) : null}
    </fieldset>
  );
}
