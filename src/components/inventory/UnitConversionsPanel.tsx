/**
 * Unit conversions of stock units (Phase 6.8): `1 from unit = factor × to unit`, for every
 * article or for one article; the inverse is derived. Includes a converter to check a quantity.
 */

import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { usePermissions } from "@/hooks/use-permissions";
import {
  type ConvertedQuantity,
  type UnitConversion,
  convertUnitQuantity,
  createUnitConversion,
  deactivateUnitConversion,
  listUnitConversions,
} from "@/services/unit-conversion-service";
import { toErrorMessage } from "@/utils/errors";
import type { InventoryArticle, LookupValueOption } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const selectClass = "h-9 rounded-md border border-surface-border bg-surface-1 px-2 text-sm";

/** Positive finite number from a text field (comma or dot), or null. */
export function parseFactor(raw: string): number | null {
  if (raw.trim() === "") return null;
  const n = Number(raw.replace(",", "."));
  return Number.isFinite(n) && n > 0 ? n : null;
}

interface Props {
  unitOptions: LookupValueOption[];
  articles: InventoryArticle[];
}

export function UnitConversionsPanel({ unitOptions, articles }: Props) {
  const { t } = useTranslation("inventory");
  const { can } = usePermissions();
  const canManage = can(P.INV_MANAGE);
  const [rows, setRows] = useState<UnitConversion[]>([]);
  const [error, setError] = useState<string | null>(null);
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
    setError(null);
    try {
      setRows(await listUnitConversions(null));
    } catch (e) {
      setError(toErrorMessage(e));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const factorValue = parseFactor(factor);
  const canCreate = from !== "" && to !== "" && from !== to && factorValue != null && !busy;
  const articleId = article === "" ? null : Number(article);

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
      setError(toErrorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const onDeactivate = async (row: UnitConversion) => {
    setError(null);
    try {
      await deactivateUnitConversion(row.id, row.row_version);
      await load();
    } catch (e) {
      setError(toErrorMessage(e));
    }
  };

  const onConvert = async () => {
    setConvertError(null);
    setResult(null);
    const q = Number(qty.replace(",", "."));
    if (!Number.isFinite(q) || from === "" || to === "") return;
    try {
      setResult(await convertUnitQuantity(q, Number(from), Number(to), articleId));
    } catch (e) {
      setConvertError(toErrorMessage(e));
    }
  };

  const unitCode = (id: string) => unitOptions.find((u) => String(u.id) === id)?.code ?? "";

  return (
    <div className="space-y-4 text-sm">
      <p className="text-xs text-text-muted">{t("unitConversions.help")}</p>
      {error ? (
        <p role="alert" className="text-xs text-text-danger">
          {error}
        </p>
      ) : null}

      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-1">
          <span className="text-xs text-text-muted">{t("unitConversions.from")}</span>
          <select className={selectClass} value={from} onChange={(e) => setFrom(e.target.value)}>
            <option value="">—</option>
            {unitOptions.map((u) => (
              <option key={u.id} value={String(u.id)}>
                {u.code} — {u.label}
              </option>
            ))}
          </select>
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
          <select className={selectClass} value={to} onChange={(e) => setTo(e.target.value)}>
            <option value="">—</option>
            {unitOptions.map((u) => (
              <option key={u.id} value={String(u.id)}>
                {u.code} — {u.label}
              </option>
            ))}
          </select>
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
      {from !== "" && to !== "" && factorValue != null ? (
        <p className="text-xs text-text-secondary">
          {t("unitConversions.preview", {
            from: unitCode(from),
            to: unitCode(to),
            factor: factorValue,
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
              quantity: result.quantity,
              unit: unitCode(to),
              path: t(`unitConversions.path.${result.path}`),
            })}
          </span>
        ) : null}
        {convertError ? (
          <span role="alert" className="text-xs text-text-danger">
            {convertError}
          </span>
        ) : null}
      </div>

      <table className="w-full text-left text-xs">
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
                  1 {r.from_unit_code} = {r.factor} {r.to_unit_code}
                </td>
                <td className="py-1">{r.article_code ?? t("unitConversions.allArticles")}</td>
                <td className="py-1">{r.notes ?? ""}</td>
                <td className="py-1 text-right">
                  {canManage ? (
                    <Button size="sm" variant="ghost" onClick={() => void onDeactivate(r)}>
                      {t("unitConversions.deactivate")}
                    </Button>
                  ) : null}
                </td>
              </tr>
            ))
          )}
        </tbody>
      </table>
    </div>
  );
}
