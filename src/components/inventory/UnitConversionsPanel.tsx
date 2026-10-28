/**
 * Unit conversions (Phase 6.8): `1 from unit = factor × to unit` between units of the governed
 * list MEASURE.UNITS, for one article or (between standard units) for every article; the
 * inverse is derived. The general factor of a plant unit (1 bag = 25 kg) is set on the unit in
 * Données de référence; the article's own conversion wins over it. Includes a converter to check
 * a quantity with the same rule the server applies.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  MeasureUnitSelect,
  isStandardUnit,
  unitSymbol,
} from "@/components/lookups/MeasureUnitPicker";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { mfInput } from "@/design-system/tokens";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { appErrorReason } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import {
  type ConvertedQuantity,
  type UnitConversion,
  convertUnitQuantity,
  createUnitConversion,
  deactivateUnitConversion,
  listUnitConversions,
} from "@/services/unit-conversion-service";
import type { InventoryArticle, ReferenceValue } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const selectClass = cn(mfInput.nativeSelect, "h-9 w-auto");

/** Positive finite number from a text field (comma or dot), or null. */
export function parseFactor(raw: string): number | null {
  if (raw.trim() === "") return null;
  const n = Number(raw.replace(",", "."));
  return Number.isFinite(n) && n > 0 ? n : null;
}

interface Props {
  /** Active units of MEASURE.UNITS. */
  unitOptions: ReferenceValue[];
  articles: InventoryArticle[];
}

export function UnitConversionsPanel({ unitOptions, articles }: Props) {
  const { t } = useTranslation("inventory");
  const { can } = usePermissions();
  const { formatNumber } = useFormatters();
  const canManage = can(P.INV_MANAGE);
  const [rows, setRows] = useState<UnitConversion[]>([]);
  /** The list could not be read (never shown as "no conversion"). */
  const [loadError, setLoadError] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [pendingDeactivate, setPendingDeactivate] = useState<UnitConversion | null>(null);
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [article, setArticle] = useState("");
  const [factor, setFactor] = useState("");
  const [notes, setNotes] = useState("");
  const [busy, setBusy] = useState(false);
  const [qty, setQty] = useState("1");
  const [result, setResult] = useState<ConvertedQuantity | null>(null);
  const [convertError, setConvertError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      setRows(await listUnitConversions(null));
    } catch (e) {
      setLoadError(e);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const factorValue = parseFactor(factor);
  const articleId = article === "" ? null : Number(article);
  const unitById = (id: string) => unitOptions.find((u) => String(u.id) === id);
  // A general conversion is only stored between standard units: the general factor of a plant
  // unit is the one set on the unit (the server refuses the other case too).
  const plantUnitInGeneralRule =
    articleId == null &&
    [from, to].some((id) => {
      const u = unitById(id);
      return u != null && !isStandardUnit(u);
    });
  const canCreate =
    from !== "" &&
    to !== "" &&
    from !== to &&
    factorValue != null &&
    !plantUnitInGeneralRule &&
    !busy;

  const onCreate = async () => {
    if (!canCreate || factorValue == null) return;
    setBusy(true);
    setError(null);
    try {
      await createUnitConversion({
        from_unit_value_id: Number(from),
        to_unit_value_id: Number(to),
        article_id: articleId,
        factor: factorValue,
        notes: notes.trim() === "" ? null : notes.trim(),
      });
      setFactor("");
      setNotes("");
      await load();
    } catch (e) {
      setError(appErrorReason(e));
    } finally {
      setBusy(false);
    }
  };

  /** Rejects on failure so the confirmation stays open (the global toast explains why). */
  const onDeactivate = async (row: UnitConversion) => {
    setError(null);
    await deactivateUnitConversion(row.id, row.row_version);
    await load();
  };

  const onConvert = async () => {
    setConvertError(null);
    setResult(null);
    const q = Number(qty.replace(",", "."));
    if (!Number.isFinite(q) || from === "" || to === "") return;
    try {
      setResult(await convertUnitQuantity(q, Number(from), Number(to), articleId));
    } catch (e) {
      setConvertError(appErrorReason(e));
    }
  };

  const unitCode = (id: string) => {
    const u = unitById(id);
    return u ? unitSymbol(u) : "";
  };

  return (
    <div className="space-y-4 text-sm">
      <p className="text-xs text-text-muted">{t("unitConversions.help")}</p>
      {error ? (
        <p role="alert" className="text-xs text-tone-danger-fg">
          {error}
        </p>
      ) : null}

      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-muted">{t("unitConversions.from")}</span>
          <MeasureUnitSelect
            className={selectClass}
            value={from === "" ? null : Number(from)}
            units={unitOptions}
            onChange={(id) => setFrom(id == null ? "" : String(id))}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-muted">{t("unitConversions.factor")}</span>
          <Input
            className="w-28"
            inputMode="decimal"
            value={factor}
            onChange={(e) => setFactor(e.target.value)}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-muted">{t("unitConversions.to")}</span>
          <MeasureUnitSelect
            className={selectClass}
            value={to === "" ? null : Number(to)}
            units={unitOptions}
            onChange={(id) => setTo(id == null ? "" : String(id))}
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-muted">{t("unitConversions.article")}</span>
          <select
            className={selectClass}
            value={article}
            onChange={(e) => setArticle(e.target.value)}
          >
            <option value="">{t("unitConversions.allArticles")}</option>
            {articles.map((a) => (
              <option key={a.id} value={String(a.id)}>
                {a.article_code} — {a.article_name}
              </option>
            ))}
          </select>
        </label>
        {canManage ? (
          <>
            <label className="flex flex-col gap-1">
              <span className="text-xs text-text-muted">{t("unitConversions.notes")}</span>
              <Input className="w-48" value={notes} onChange={(e) => setNotes(e.target.value)} />
            </label>
            <Button size="sm" disabled={!canCreate} onClick={() => void onCreate()}>
              {t("unitConversions.add")}
            </Button>
          </>
        ) : null}
      </div>
      {canManage && plantUnitInGeneralRule ? (
        <p className="text-xs text-text-secondary">{t("unitConversions.plantUnitGeneral")}</p>
      ) : null}
      {from !== "" && to !== "" && factorValue != null ? (
        <p className="text-xs text-text-secondary">
          {t("unitConversions.preview", {
            from: unitCode(from),
            to: unitCode(to),
            factor: formatNumber(factorValue, { maximumFractionDigits: 6 }),
          })}
        </p>
      ) : null}

      <div className="flex flex-wrap items-end gap-2 rounded-md border border-surface-border p-2">
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-muted">{t("unitConversions.quantity")}</span>
          <Input
            className="w-28"
            inputMode="decimal"
            value={qty}
            onChange={(e) => setQty(e.target.value)}
          />
        </label>
        <Button
          size="sm"
          variant="outline"
          disabled={from === "" || to === ""}
          onClick={() => void onConvert()}
        >
          {t("unitConversions.convert")}
        </Button>
        {result ? (
          <span data-testid="conversion-result" className="text-xs">
            {t("unitConversions.result", {
              quantity: formatNumber(result.quantity, { maximumFractionDigits: 6 }),
              unit: unitCode(to),
              path: t(`unitConversions.path.${result.path}`),
            })}
          </span>
        ) : null}
        {convertError ? (
          <span role="alert" className="text-xs text-tone-danger-fg">
            {convertError}
          </span>
        ) : null}
      </div>

      {loadError != null ? (
        <ErrorState
          error={loadError}
          title={t("unitConversions.loadError")}
          onRetry={() => void load()}
          className="py-4"
        />
      ) : null}
      <table className={loadError != null ? "hidden" : "w-full text-left text-xs"}>
        <thead className="text-text-muted">
          <tr>
            <th className="py-1">{t("unitConversions.rule")}</th>
            <th className="py-1">{t("unitConversions.article")}</th>
            <th className="py-1">{t("unitConversions.notes")}</th>
            <th className="py-1" />
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr>
              <td colSpan={4} className="py-3 text-center text-text-muted">
                {t("unitConversions.empty")}
              </td>
            </tr>
          ) : (
            rows.map((r) => (
              <tr key={r.id} className="border-t border-surface-border">
                <td className="py-1 font-mono">
                  1 {r.from_unit_code} = {formatNumber(r.factor, { maximumFractionDigits: 6 })}{" "}
                  {r.to_unit_code}
                </td>
                <td className="py-1">{r.article_code ?? t("unitConversions.allArticles")}</td>
                <td className="py-1">{r.notes ?? ""}</td>
                <td className="py-1 text-right">
                  {canManage ? (
                    <Button
                      size="sm"
                      variant="ghost"
                      className="h-9"
                      onClick={() => setPendingDeactivate(r)}
                    >
                      {t("unitConversions.deactivate")}
                    </Button>
                  ) : null}
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
      <ConfirmDialog
        open={pendingDeactivate != null}
        onOpenChange={(open) => {
          if (!open) setPendingDeactivate(null);
        }}
        title={t("unitConversions.deactivateTitle")}
        description={t("unitConversions.deactivateDescription", {
          rule: pendingDeactivate
            ? `1 ${pendingDeactivate.from_unit_code} = ${formatNumber(pendingDeactivate.factor, {
                maximumFractionDigits: 6,
              })} ${pendingDeactivate.to_unit_code}`
            : "",
        })}
        confirmLabel={t("unitConversions.deactivate")}
        destructive
        onConfirm={async () => {
          if (pendingDeactivate) await onDeactivate(pendingDeactivate);
        }}
      />
    </div>
  );
}
