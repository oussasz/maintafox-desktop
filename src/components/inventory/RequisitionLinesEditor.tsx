/**
 * Lines of a purchase requisition: article, delivery location, quantity and estimated unit cost
 * (tenant currency). The estimated total sets the amount checked against approval limits.
 */
import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { DecimalField } from "@/components/ui/decimal-field";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useFormatters } from "@/hooks/use-formatters";
import type { InventoryArticleRow, RequisitionLineInput } from "@/services/inventory-service";
import type { StockLocation } from "@shared/ipc-types";

export interface RequisitionDraftLine {
  key: string;
  articleId: number;
  locationId: number;
  qty: number;
  /** Typed estimated unit cost; empty = not estimated. */
  estimatedCost: string;
}

let lineSeq = 0;

export function emptyRequisitionLine(
  partial: Partial<RequisitionDraftLine> = {},
): RequisitionDraftLine {
  lineSeq += 1;
  return {
    key: `line-${lineSeq}`,
    articleId: 0,
    locationId: 0,
    qty: 0,
    estimatedCost: "",
    ...partial,
  };
}

function parseCost(value: string): number | null {
  const trimmed = value.trim().replace(",", ".");
  if (trimmed === "") return null;
  const n = Number(trimmed);
  return Number.isFinite(n) ? n : Number.NaN;
}

/** First problem that prevents saving the lines, or null. */
export function requisitionLinesBlocker(
  lines: RequisitionDraftLine[],
): "article" | "quantity" | "cost" | null {
  if (lines.length === 0 || lines.some((l) => l.articleId <= 0)) return "article";
  if (lines.some((l) => !(l.qty > 0))) return "quantity";
  if (
    lines.some((l) => {
      const cost = parseCost(l.estimatedCost);
      return cost !== null && !(cost >= 0);
    })
  )
    return "cost";
  return null;
}

export function draftLinesToInput(lines: RequisitionDraftLine[]): RequisitionLineInput[] {
  return lines.map((l) => ({
    article_id: l.articleId,
    preferred_location_id: l.locationId > 0 ? l.locationId : null,
    requested_qty: l.qty,
    estimated_unit_cost: parseCost(l.estimatedCost),
  }));
}

/** Estimated total over the estimated lines; `complete` is false when a line has no estimate. */
export function draftTotal(lines: RequisitionDraftLine[]): { total: number; complete: boolean } {
  let total = 0;
  let complete = true;
  for (const l of lines) {
    const cost = parseCost(l.estimatedCost);
    if (cost === null || Number.isNaN(cost)) complete = false;
    else total += cost * (l.qty > 0 ? l.qty : 0);
  }
  return { total, complete };
}

export interface RequisitionLinesEditorProps {
  lines: RequisitionDraftLine[];
  onChange: (lines: RequisitionDraftLine[]) => void;
  articles: InventoryArticleRow[];
  locations: StockLocation[];
  disabled?: boolean;
}

export function RequisitionLinesEditor({
  lines,
  onChange,
  articles,
  locations,
  disabled = false,
}: RequisitionLinesEditorProps) {
  const { t } = useTranslation("inventory");
  const { formatNumber, formatCurrency, currencyCode } = useFormatters();
  const qtyLabel = (v: number) => formatNumber(v, { maximumFractionDigits: 3 });

  const update = (key: string, patch: Partial<RequisitionDraftLine>) =>
    onChange(lines.map((l) => (l.key === key ? { ...l, ...patch } : l)));

  const chooseArticle = (line: RequisitionDraftLine, articleId: number) => {
    const article = articles.find((a) => a.id === articleId);
    update(line.key, {
      articleId,
      // Start from the article's average cost when nothing was typed yet.
      ...(line.estimatedCost === "" && article?.avg_unit_cost != null
        ? { estimatedCost: String(Math.round(article.avg_unit_cost * 100) / 100) }
        : {}),
      ...(line.locationId === 0 && article?.preferred_location_id
        ? { locationId: article.preferred_location_id }
        : {}),
    });
  };

  const { total, complete } = draftTotal(lines);

  return (
    <div className="space-y-3">
      {lines.map((line, index) => {
        const article = articles.find((a) => a.id === line.articleId) ?? null;
        const id = (field: string) => `inv-req-${field}-${line.key}`;
        const cost = parseCost(line.estimatedCost);
        return (
          <fieldset
            key={line.key}
            className="grid gap-3 rounded-md border border-surface-border p-3 md:grid-cols-[2fr_1.4fr_1fr_1fr_auto]"
          >
            <legend className="px-1 text-xs font-medium text-text-muted">
              {t("procurement.requisitionForm.lineLabel", { n: index + 1 })}
            </legend>
            <div className="space-y-1">
              <Label htmlFor={id("article")}>{t("procurement.requisitionForm.article")}</Label>
              <Select
                value={String(line.articleId)}
                onValueChange={(v) => v && chooseArticle(line, Number(v))}
                disabled={disabled}
              >
                <SelectTrigger id={id("article")} className="h-11">
                  <SelectValue placeholder={t("procurement.requisitionForm.selectArticle")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">
                    {t("procurement.requisitionForm.selectArticle")}
                  </SelectItem>
                  {articles.map((a) => (
                    <SelectItem key={a.id} value={String(a.id)}>
                      {a.article_code} - {a.article_name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              {article ? (
                <p className="text-xs text-text-muted">
                  {t("procurement.requisitionForm.stockHint", {
                    available: qtyLabel(article.available_qty),
                    reorderPoint: qtyLabel(article.reorder_point),
                    unit: article.unit_code,
                  })}
                </p>
              ) : null}
            </div>
            <div className="space-y-1">
              <Label htmlFor={id("location")}>{t("procurement.requisitionForm.location")}</Label>
              <Select
                value={String(line.locationId)}
                onValueChange={(v) => v && update(line.key, { locationId: Number(v) })}
                disabled={disabled}
              >
                <SelectTrigger id={id("location")} className="h-11">
                  <SelectValue placeholder={t("procurement.requisitionForm.noLocation")} />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="0">{t("procurement.requisitionForm.noLocation")}</SelectItem>
                  {locations.map((l) => (
                    <SelectItem key={l.id} value={String(l.id)}>
                      {l.warehouse_code}/{l.code}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="space-y-1">
              <Label htmlFor={id("qty")}>{t("procurement.requisitionForm.quantity")}</Label>
              <DecimalField
                id={id("qty")}
                decimals={3}
                min={0}
                className="h-11"
                value={line.qty}
                disabled={disabled}
                onValueChange={(v) => update(line.key, { qty: v ?? 0 })}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor={id("cost")}>
                {t("procurement.requisitionForm.estimatedCost", { currency: currencyCode })}
              </Label>
              <DecimalField
                id={id("cost")}
                decimals={2}
                fixed
                min={0}
                className="h-11"
                value={cost !== null && !Number.isNaN(cost) ? cost : null}
                disabled={disabled}
                onValueChange={(v) =>
                  update(line.key, { estimatedCost: v == null ? "" : String(v) })
                }
              />
              {cost !== null && !Number.isNaN(cost) && line.qty > 0 ? (
                <p className="text-xs tabular-nums text-text-muted">
                  {t("procurement.requisitionForm.lineTotal", {
                    total: formatCurrency(cost * line.qty),
                  })}
                </p>
              ) : null}
            </div>
            <div className="flex items-end">
              <Button
                type="button"
                variant="ghost"
                className="h-11 w-11 p-0"
                disabled={disabled || lines.length === 1}
                aria-label={t("procurement.requisitionForm.removeLine", { n: index + 1 })}
                onClick={() => onChange(lines.filter((l) => l.key !== line.key))}
              >
                <Trash2 className="h-4 w-4" aria-hidden />
              </Button>
            </div>
          </fieldset>
        );
      })}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <Button
          type="button"
          variant="outline"
          className="h-11"
          disabled={disabled}
          onClick={() => onChange([...lines, emptyRequisitionLine()])}
        >
          <Plus className="mr-1 h-4 w-4" aria-hidden />
          {t("procurement.requisitionForm.addLine")}
        </Button>
        <p className="text-sm tabular-nums" role="status">
          {complete
            ? t("procurement.requisitionForm.total", { total: formatCurrency(total) })
            : t("procurement.requisitionForm.totalIncomplete", { total: formatCurrency(total) })}
        </p>
      </div>
      <p className="text-xs text-text-muted">
        {t("procurement.requisitionForm.estimatedCostHint")}
      </p>
    </div>
  );
}
