import { Loader2 } from "lucide-react";
import { type Dispatch, type SetStateAction, useState } from "react";
import { useTranslation } from "react-i18next";

import { unitOptionText } from "@/components/lookups/MeasureUnitPicker";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { InventoryArticleInputFull } from "@/services/inventory-service";
import type {
  ArticleFamily,
  LookupValueOption,
  ReferenceValue,
  StockLocation,
  Warehouse,
} from "@shared/ipc-types";

/** Article form state: the full item-master payload accepted by the backend. */
export type ExtendedArticleInput = InventoryArticleInputFull;

export type ArticleEditorFieldsProps = {
  articleForm: ExtendedArticleInput;
  setArticleForm: Dispatch<SetStateAction<ExtendedArticleInput>>;
  families: ArticleFamily[];
  warehouses: Warehouse[];
  preferredWarehouseLocations: StockLocation[];
  /** Active units of the governed list MEASURE.UNITS. */
  unitOptions: ReferenceValue[];
  criticalityOptions: LookupValueOption[];
  stockingTypeOptions: LookupValueOption[];
  taxCategoryOptions: LookupValueOption[];
  procurementCategoryOptions: LookupValueOption[];
  /**
   * Recalculates the ABC or XYZ class of every article from its last 12 months of movements
   * (editing an existing article only). The page reports the result and reloads the article.
   */
  onRecalculateClasses?: (kind: "abc" | "xyz") => Promise<void>;
};

const REPLENISHMENT_POLICIES = ["EOQ", "MIN_MAX", "PERIODIC", "KANBAN", "ON_DEMAND"] as const;

/** Label + control pair with an explicit `htmlFor` so every field has an accessible name. */
function Field({
  id,
  label,
  className,
  children,
}: {
  id: string;
  label: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={className ?? "space-y-1"}>
      <label htmlFor={id} className="text-xs text-text-muted">
        {label}
      </label>
      {children}
    </div>
  );
}

function SectionTitle({ children }: { children: React.ReactNode }) {
  return (
    <div className="col-span-full mt-2 border-t pt-3">
      <h4 className="text-xs font-semibold uppercase tracking-wide text-text-muted">{children}</h4>
    </div>
  );
}

export function ArticleEditorFields({
  articleForm,
  setArticleForm,
  families,
  warehouses,
  preferredWarehouseLocations,
  unitOptions,
  criticalityOptions,
  stockingTypeOptions,
  taxCategoryOptions,
  procurementCategoryOptions,
  onRecalculateClasses,
}: ArticleEditorFieldsProps) {
  const { t, i18n } = useTranslation("inventory");
  const [recalculating, setRecalculating] = useState<"abc" | "xyz" | null>(null);

  const recalculate = async (kind: "abc" | "xyz") => {
    if (!onRecalculateClasses) return;
    setRecalculating(kind);
    try {
      await onRecalculateClasses(kind);
    } finally {
      setRecalculating(null);
    }
  };

  return (
    <div className="grid gap-3 md:grid-cols-3">
      {/* ── CLASSIFICATION ── */}
      <Field id="article-code" label={t("article.form.code")}>
        <Input
          id="article-code"
          placeholder={t("article.form.codePlaceholder")}
          value={articleForm.article_code}
          onChange={(e) => setArticleForm((s) => ({ ...s, article_code: e.target.value }))}
        />
      </Field>
      <Field id="article-name" label={t("article.form.name")} className="space-y-1 md:col-span-2">
        <Input
          id="article-name"
          placeholder={t("article.form.namePlaceholder")}
          value={articleForm.article_name}
          onChange={(e) => setArticleForm((s) => ({ ...s, article_name: e.target.value }))}
        />
      </Field>
      <Field id="article-family" label={t("detail.fields.family")}>
        <Select
          value={String(articleForm.family_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, family_id: v === "__none__" ? null : Number(v) }))
          }
        >
          <SelectTrigger id="article-family">
            <SelectValue placeholder={t("article.form.noFamily")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("article.form.noFamily")}</SelectItem>
            {families.map((f) => (
              <SelectItem key={f.id} value={String(f.id)}>
                {f.code} - {f.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field id="article-unit" label={t("article.form.unit")}>
        <Select
          value={String(articleForm.unit_value_id)}
          onValueChange={(v) => setArticleForm((s) => ({ ...s, unit_value_id: Number(v) }))}
        >
          <SelectTrigger id="article-unit">
            <SelectValue placeholder={t("article.form.selectUnit")} />
          </SelectTrigger>
          <SelectContent>
            {unitOptions.map((u) => (
              <SelectItem key={u.id} value={String(u.id)}>
                {unitOptionText(u, i18n.language)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field id="article-criticality" label={t("detail.fields.criticality")}>
        <Select
          value={String(articleForm.criticality_value_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              criticality_value_id: v === "__none__" ? null : Number(v),
            }))
          }
        >
          <SelectTrigger id="article-criticality">
            <SelectValue placeholder={t("article.form.noCriticality")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("article.form.noCriticality")}</SelectItem>
            {criticalityOptions.map((c) => (
              <SelectItem key={c.id} value={String(c.id)}>
                {c.code} - {c.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field id="article-stocking-type" label={t("detail.fields.stockingType")}>
        <Select
          value={String(articleForm.stocking_type_value_id)}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, stocking_type_value_id: Number(v) }))
          }
        >
          <SelectTrigger id="article-stocking-type">
            <SelectValue placeholder={t("article.form.selectStockingType")} />
          </SelectTrigger>
          <SelectContent>
            {stockingTypeOptions.map((item) => (
              <SelectItem key={item.id} value={String(item.id)}>
                {item.code} - {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field id="article-tax-category" label={t("detail.fields.taxCategory")}>
        <Select
          value={String(articleForm.tax_category_value_id)}
          onValueChange={(v) => setArticleForm((s) => ({ ...s, tax_category_value_id: Number(v) }))}
        >
          <SelectTrigger id="article-tax-category">
            <SelectValue placeholder={t("article.form.selectTaxCategory")} />
          </SelectTrigger>
          <SelectContent>
            {taxCategoryOptions.map((item) => (
              <SelectItem key={item.id} value={String(item.id)}>
                {item.code} - {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>
      <Field id="article-procurement-category" label={t("article.form.procurementCategory")}>
        <Select
          value={String(articleForm.procurement_category_value_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              procurement_category_value_id: v === "__none__" ? null : Number(v),
            }))
          }
        >
          <SelectTrigger id="article-procurement-category">
            <SelectValue placeholder={t("article.form.noProcurementCategory")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("article.form.noProcurementCategory")}</SelectItem>
            {procurementCategoryOptions.map((item) => (
              <SelectItem key={item.id} value={String(item.id)}>
                {item.code} - {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      {/* ── WAREHOUSE PREFERENCES ── */}
      <Field id="article-preferred-warehouse" label={t("article.form.preferredWarehouse")}>
        <Select
          value={String(articleForm.preferred_warehouse_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              preferred_warehouse_id: v === "__none__" ? null : Number(v),
              preferred_location_id: null,
            }))
          }
        >
          <SelectTrigger id="article-preferred-warehouse">
            <SelectValue placeholder={t("article.form.noPreferredWarehouse")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("article.form.noPreferredWarehouse")}</SelectItem>
            {warehouses
              .filter((w) => w.is_active === 1)
              .map((w) => (
                <SelectItem key={w.id} value={String(w.id)}>
                  {w.code} - {w.name}
                </SelectItem>
              ))}
          </SelectContent>
        </Select>
      </Field>
      <Field id="article-preferred-location" label={t("article.form.preferredLocation")}>
        <Select
          value={String(articleForm.preferred_location_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              preferred_location_id: v === "__none__" ? null : Number(v),
            }))
          }
          disabled={articleForm.preferred_warehouse_id === null}
        >
          <SelectTrigger id="article-preferred-location">
            <SelectValue placeholder={t("article.form.noPreferredLocation")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("article.form.noPreferredLocation")}</SelectItem>
            {preferredWarehouseLocations.map((location) => (
              <SelectItem key={location.id} value={String(location.id)}>
                {location.warehouse_code}/{location.code} - {location.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        {articleForm.preferred_warehouse_id === null ? (
          <p className="text-xs text-text-muted">{t("article.form.chooseWarehouseFirst")}</p>
        ) : null}
      </Field>

      {/* ── STOCK PARAMETERS ── */}
      <Field id="article-min-stock" label={t("detail.fields.minStock")}>
        <Input
          id="article-min-stock"
          type="number"
          step="0.01"
          min={0}
          value={articleForm.min_stock}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, min_stock: Number(e.target.value || 0) }))
          }
        />
      </Field>
      <Field id="article-reorder-point" label={t("detail.fields.reorderPoint")}>
        <Input
          id="article-reorder-point"
          type="number"
          step="0.01"
          min={0}
          value={articleForm.reorder_point}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, reorder_point: Number(e.target.value || 0) }))
          }
        />
      </Field>
      <Field id="article-max-stock" label={t("article.form.maxStock")}>
        <Input
          id="article-max-stock"
          type="number"
          step="0.01"
          min={0}
          placeholder={t("article.form.maxStockPlaceholder")}
          value={articleForm.max_stock ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              max_stock: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </Field>
      <Field id="article-safety-stock" label={t("detail.fields.safetyStock")}>
        <Input
          id="article-safety-stock"
          type="number"
          step="0.01"
          min={0}
          value={articleForm.safety_stock}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, safety_stock: Number(e.target.value || 0) }))
          }
        />
      </Field>

      {/* ── IDENTITY (Phase 2) ── */}
      <SectionTitle>{t("article.sections.identity")}</SectionTitle>
      <div className="space-y-1">
        <label htmlFor="article-field-manufacturerName" className="text-xs text-text-muted">
          {t("article.fields.manufacturerName")}
        </label>
        <Input
          id="article-field-manufacturerName"
          value={articleForm.manufacturer_name ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, manufacturer_name: e.target.value || null }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-manufacturerPartNumber" className="text-xs text-text-muted">
          {t("article.fields.manufacturerPartNumber")}
        </label>
        <Input
          id="article-field-manufacturerPartNumber"
          value={articleForm.manufacturer_part_number ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, manufacturer_part_number: e.target.value || null }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-oemPartNumber" className="text-xs text-text-muted">
          {t("article.fields.oemPartNumber")}
        </label>
        <Input
          id="article-field-oemPartNumber"
          value={articleForm.oem_part_number ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, oem_part_number: e.target.value || null }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-barcode" className="text-xs text-text-muted">
          {t("article.fields.barcode")}
        </label>
        <Input
          id="article-field-barcode"
          className="font-mono"
          autoComplete="off"
          value={articleForm.barcode ?? ""}
          aria-describedby="article-field-barcode-hint"
          onChange={(e) => setArticleForm((s) => ({ ...s, barcode: e.target.value || null }))}
        />
        <p id="article-field-barcode-hint" className="text-xs text-text-muted">
          {t("article.fields.barcodeHint")}
        </p>
      </div>

      {/* ── REPLENISHMENT (Phase 2) ── */}
      <SectionTitle>{t("article.sections.replenishment")}</SectionTitle>
      <div className="space-y-1">
        <label htmlFor="article-field-replenishmentPolicy" className="text-xs text-text-muted">
          {t("article.fields.replenishmentPolicy")}
        </label>
        <Select
          value={articleForm.replenishment_policy_code ?? "__none__"}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              replenishment_policy_code: v === "__none__" ? null : v,
            }))
          }
        >
          <SelectTrigger id="article-field-replenishmentPolicy">
            <SelectValue placeholder={t("article.form.noPolicy")} />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">{t("article.form.noPolicy")}</SelectItem>
            {REPLENISHMENT_POLICIES.map((p) => (
              <SelectItem key={p} value={p}>
                {t(`article.replenishmentPolicies.${p}`)}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-eoq" className="text-xs text-text-muted">
          {t("article.fields.eoq")}
        </label>
        <Input
          id="article-field-eoq"
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.economic_order_qty ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              economic_order_qty: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-moq" className="text-xs text-text-muted">
          {t("article.fields.moq")}
        </label>
        <Input
          id="article-field-moq"
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.minimum_order_qty ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              minimum_order_qty: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-maxOrder" className="text-xs text-text-muted">
          {t("article.fields.maxOrder")}
        </label>
        <Input
          id="article-field-maxOrder"
          type="number"
          step="0.01"
          min={0}
          value={articleForm.maximum_order_qty ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              maximum_order_qty: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-orderMultiple" className="text-xs text-text-muted">
          {t("article.fields.orderMultiple")}
        </label>
        <Input
          id="article-field-orderMultiple"
          type="number"
          step="0.01"
          min={0}
          value={articleForm.order_multiple_qty ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              order_multiple_qty: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-leadTimeDays" className="text-xs text-text-muted">
          {t("article.fields.leadTimeDays")}
        </label>
        <Input
          id="article-field-leadTimeDays"
          type="number"
          min={0}
          value={articleForm.lead_time_days ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              lead_time_days: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-reviewPeriodDays" className="text-xs text-text-muted">
          {t("article.fields.reviewPeriodDays")}
        </label>
        <Input
          id="article-field-reviewPeriodDays"
          type="number"
          min={0}
          value={articleForm.review_period_days ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              review_period_days: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-abcClass" className="text-xs text-text-muted">
          {t("article.fields.abcClass")}
        </label>
        <Select
          value={articleForm.abc_class_code ?? "__none__"}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, abc_class_code: v === "__none__" ? null : v }))
          }
        >
          <SelectTrigger id="article-field-abcClass">
            <SelectValue placeholder="—" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">—</SelectItem>
            <SelectItem value="A">A</SelectItem>
            <SelectItem value="B">B</SelectItem>
            <SelectItem value="C">C</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label htmlFor="article-field-xyzClass" className="text-xs text-text-muted">
          {t("article.fields.xyzClass")}
        </label>
        <Select
          value={articleForm.xyz_class_code ?? "__none__"}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, xyz_class_code: v === "__none__" ? null : v }))
          }
        >
          <SelectTrigger id="article-field-xyzClass">
            <SelectValue placeholder="—" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">—</SelectItem>
            <SelectItem value="X">X</SelectItem>
            <SelectItem value="Y">Y</SelectItem>
            <SelectItem value="Z">Z</SelectItem>
          </SelectContent>
        </Select>
      </div>
      {/* ABC/XYZ recalculation (all articles, from movements) — editing only */}
      {onRecalculateClasses ? (
        <div className="col-span-full space-y-1">
          <div className="flex flex-wrap gap-2">
            <Button
              type="button"
              variant="outline"
              className="h-10 gap-1.5"
              disabled={recalculating !== null}
              onClick={() => void recalculate("abc")}
            >
              {recalculating === "abc" ? (
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
              ) : null}
              {t("article.actions.calculateAbc")}
            </Button>
            <Button
              type="button"
              variant="outline"
              className="h-10 gap-1.5"
              disabled={recalculating !== null}
              onClick={() => void recalculate("xyz")}
            >
              {recalculating === "xyz" ? (
                <Loader2 className="h-4 w-4 animate-spin" aria-hidden />
              ) : null}
              {t("article.actions.calculateXyz")}
            </Button>
          </div>
          <p className="text-xs text-text-muted">{t("article.classification.hint")}</p>
        </div>
      ) : null}

      {/* ── CRITICAL SPARE (Phase 2) ── */}
      <SectionTitle>{t("article.sections.criticalSpare")}</SectionTitle>
      <div className="col-span-full flex items-center gap-2">
        <input
          id="is-critical-spare"
          type="checkbox"
          checked={articleForm.is_critical_spare ?? false}
          onChange={(e) => setArticleForm((s) => ({ ...s, is_critical_spare: e.target.checked }))}
        />
        <label htmlFor="is-critical-spare" className="text-sm">
          {t("article.fields.isCriticalSpare")}
        </label>
      </div>

      {/* ── SHELF LIFE (Phase 2) ── */}
      <SectionTitle>{t("article.sections.shelfLife")}</SectionTitle>
      <div className="col-span-full flex flex-wrap items-center gap-4">
        <div className="flex items-center gap-2">
          <input
            id="requires-expiration"
            type="checkbox"
            checked={articleForm.requires_expiration ?? false}
            onChange={(e) =>
              setArticleForm((s) => ({ ...s, requires_expiration: e.target.checked }))
            }
          />
          <label htmlFor="requires-expiration" className="text-sm">
            {t("article.fields.requiresExpiration")}
          </label>
        </div>
        <div className="flex items-center gap-2">
          <input
            id="requires-batch-tracking"
            type="checkbox"
            checked={articleForm.requires_batch_tracking ?? false}
            onChange={(e) =>
              setArticleForm((s) => ({ ...s, requires_batch_tracking: e.target.checked }))
            }
          />
          <label htmlFor="requires-batch-tracking" className="text-sm">
            {t("article.fields.requiresBatchTracking")}
          </label>
        </div>
        <div className="flex items-center gap-2">
          <input
            id="requires-serial-tracking"
            type="checkbox"
            checked={articleForm.requires_serial_tracking ?? false}
            onChange={(e) =>
              setArticleForm((s) => ({ ...s, requires_serial_tracking: e.target.checked }))
            }
          />
          <label htmlFor="requires-serial-tracking" className="text-sm">
            {t("article.fields.requiresSerialTracking")}
          </label>
        </div>
      </div>
      {(articleForm.requires_expiration ?? false) ? (
        <div className="space-y-1">
          <label htmlFor="article-field-shelfLifeDays" className="text-xs text-text-muted">
            {t("article.fields.shelfLifeDays")}
          </label>
          <Input
            id="article-field-shelfLifeDays"
            type="number"
            min={0}
            value={articleForm.shelf_life_days ?? ""}
            onChange={(e) =>
              setArticleForm((s) => ({
                ...s,
                shelf_life_days: e.target.value === "" ? null : Number(e.target.value),
              }))
            }
          />
        </div>
      ) : null}
    </div>
  );
}
