import type { Dispatch, SetStateAction } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  calculateInventoryAbc,
  calculateInventoryXyz,
} from "@/services/inventory-service";
import { toErrorMessage } from "@/utils/errors";
import type { ArticleFamily, InventoryArticleInput, LookupValueOption, StockLocation, Warehouse } from "@shared/ipc-types";

/**
 * Extended form type for article editing — adds Phase 2 procurement maturity fields.
 * These extend InventoryArticleInput with optional metadata fields.
 * The backend currently accepts only InventoryArticleInput; extra fields are
 * passed through structural typing and ignored until schema is extended.
 */
export interface ExtendedArticleInput extends InventoryArticleInput {
  // Identity
  manufacturer_name?: string | null;
  manufacturer_part_number?: string | null;
  oem_part_number?: string | null;
  // Replenishment
  replenishment_policy_code?: string | null;
  eoq?: number | null;
  moq?: number | null;
  max_order_qty?: number | null;
  order_multiple?: number | null;
  lead_time_days?: number | null;
  review_period_days?: number | null;
  abc_class_code?: string | null;
  xyz_class_code?: string | null;
  // Critical spare
  is_critical_spare?: boolean | null;
  // Shelf life
  requires_expiration?: boolean | null;
  shelf_life_days?: number | null;
  requires_batch_tracking?: boolean | null;
}

export type ArticleEditorFieldsProps = {
  articleForm: ExtendedArticleInput;
  setArticleForm: Dispatch<SetStateAction<ExtendedArticleInput>>;
  families: ArticleFamily[];
  warehouses: Warehouse[];
  preferredWarehouseLocations: StockLocation[];
  unitOptions: LookupValueOption[];
  criticalityOptions: LookupValueOption[];
  stockingTypeOptions: LookupValueOption[];
  taxCategoryOptions: LookupValueOption[];
  procurementCategoryOptions: LookupValueOption[];
};

const REPLENISHMENT_POLICIES = ["EOQ", "MIN_MAX", "PERIODIC", "KANBAN", "ON_DEMAND"] as const;

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
}: ArticleEditorFieldsProps) {
  const { t } = useTranslation("inventory");

  const handleCalculateAbc = async () => {
    try {
      await calculateInventoryAbc();
    } catch (err) {
      console.error(toErrorMessage(err));
    }
  };

  const handleCalculateXyz = async () => {
    try {
      await calculateInventoryXyz();
    } catch (err) {
      console.error(toErrorMessage(err));
    }
  };

  return (
    <div className="grid gap-3 md:grid-cols-3">
      {/* ── CLASSIFICATION ── */}
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Article code</label>
        <Input
          placeholder="e.g. BRG-6205"
          value={articleForm.article_code}
          onChange={(e) => setArticleForm((s) => ({ ...s, article_code: e.target.value }))}
        />
      </div>
      <div className="space-y-1 md:col-span-2">
        <label className="text-xs text-text-muted">Article name</label>
        <Input
          placeholder="e.g. Deep groove bearing 6205"
          value={articleForm.article_name}
          onChange={(e) => setArticleForm((s) => ({ ...s, article_name: e.target.value }))}
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Family</label>
        <Select
          value={String(articleForm.family_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, family_id: v === "__none__" ? null : Number(v) }))
          }
        >
          <SelectTrigger>
            <SelectValue placeholder="Select family" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">No family</SelectItem>
            {families.map((f) => (
              <SelectItem key={f.id} value={String(f.id)}>
                {f.code} - {f.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Unit of measure</label>
        <Select
          value={String(articleForm.unit_value_id)}
          onValueChange={(v) => setArticleForm((s) => ({ ...s, unit_value_id: Number(v) }))}
        >
          <SelectTrigger>
            <SelectValue placeholder="Select unit" />
          </SelectTrigger>
          <SelectContent>
            {unitOptions.map((u) => (
              <SelectItem key={u.id} value={String(u.id)}>
                {u.code} - {u.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Criticality</label>
        <Select
          value={String(articleForm.criticality_value_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              criticality_value_id: v === "__none__" ? null : Number(v),
            }))
          }
        >
          <SelectTrigger>
            <SelectValue placeholder="Select criticality" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">No criticality</SelectItem>
            {criticalityOptions.map((c) => (
              <SelectItem key={c.id} value={String(c.id)}>
                {c.code} - {c.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Stocking type</label>
        <Select
          value={String(articleForm.stocking_type_value_id)}
          onValueChange={(v) => setArticleForm((s) => ({ ...s, stocking_type_value_id: Number(v) }))}
        >
          <SelectTrigger>
            <SelectValue placeholder="Select stocking type" />
          </SelectTrigger>
          <SelectContent>
            {stockingTypeOptions.map((item) => (
              <SelectItem key={item.id} value={String(item.id)}>
                {item.code} - {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Tax category</label>
        <Select
          value={String(articleForm.tax_category_value_id)}
          onValueChange={(v) => setArticleForm((s) => ({ ...s, tax_category_value_id: Number(v) }))}
        >
          <SelectTrigger>
            <SelectValue placeholder="Select tax category" />
          </SelectTrigger>
          <SelectContent>
            {taxCategoryOptions.map((item) => (
              <SelectItem key={item.id} value={String(item.id)}>
                {item.code} - {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Procurement category (optional)</label>
        <Select
          value={String(articleForm.procurement_category_value_id ?? "__none__")}
          onValueChange={(v) =>
            setArticleForm((s) => ({
              ...s,
              procurement_category_value_id: v === "__none__" ? null : Number(v),
            }))
          }
        >
          <SelectTrigger>
            <SelectValue placeholder="Select procurement category" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">No procurement category</SelectItem>
            {procurementCategoryOptions.map((item) => (
              <SelectItem key={item.id} value={String(item.id)}>
                {item.code} - {item.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* ── WAREHOUSE PREFERENCES ── */}
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Preferred warehouse (optional)</label>
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
          <SelectTrigger>
            <SelectValue placeholder="Select preferred warehouse" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">No preferred warehouse</SelectItem>
            {warehouses
              .filter((w) => w.is_active === 1)
              .map((w) => (
                <SelectItem key={w.id} value={String(w.id)}>
                  {w.code} - {w.name}
                </SelectItem>
              ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Preferred location (optional)</label>
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
          <SelectTrigger>
            <SelectValue placeholder="Select preferred location" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">No preferred location</SelectItem>
            {preferredWarehouseLocations.map((location) => (
              <SelectItem key={location.id} value={String(location.id)}>
                {location.warehouse_code}/{location.code} - {location.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* ── STOCK PARAMETERS ── */}
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Minimum stock</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.min_stock}
          onChange={(e) => setArticleForm((s) => ({ ...s, min_stock: Number(e.target.value || 0) }))}
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Reorder point</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.reorder_point}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, reorder_point: Number(e.target.value || 0) }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Maximum stock (optional)</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          placeholder="Leave empty if not defined"
          value={articleForm.max_stock ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              max_stock: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">Safety stock</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.safety_stock}
          onChange={(e) => setArticleForm((s) => ({ ...s, safety_stock: Number(e.target.value || 0) }))}
        />
      </div>

      {/* ── IDENTITY (Phase 2) ── */}
      <SectionTitle>{t("article.sections.identity")}</SectionTitle>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.manufacturerName")}</label>
        <Input
          value={articleForm.manufacturer_name ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, manufacturer_name: e.target.value || null }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">
          {t("article.fields.manufacturerPartNumber")}
        </label>
        <Input
          value={articleForm.manufacturer_part_number ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, manufacturer_part_number: e.target.value || null }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.oemPartNumber")}</label>
        <Input
          value={articleForm.oem_part_number ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, oem_part_number: e.target.value || null }))
          }
        />
      </div>

      {/* ── REPLENISHMENT (Phase 2) ── */}
      <SectionTitle>{t("article.sections.replenishment")}</SectionTitle>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">
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
          <SelectTrigger>
            <SelectValue placeholder="Select policy" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="__none__">No policy</SelectItem>
            {REPLENISHMENT_POLICIES.map((p) => (
              <SelectItem key={p} value={p}>
                {p}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.eoq")}</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.eoq ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              eoq: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.moq")}</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          placeholder="0"
          value={articleForm.moq ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              moq: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.maxOrder")}</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          value={articleForm.max_order_qty ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              max_order_qty: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.orderMultiple")}</label>
        <Input
          type="number"
          step="0.01"
          min={0}
          value={articleForm.order_multiple ?? ""}
          onChange={(e) =>
            setArticleForm((s) => ({
              ...s,
              order_multiple: e.target.value === "" ? null : Number(e.target.value),
            }))
          }
        />
      </div>
      <div className="space-y-1">
        <label className="text-xs text-text-muted">{t("article.fields.leadTimeDays")}</label>
        <Input
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
        <label className="text-xs text-text-muted">{t("article.fields.reviewPeriodDays")}</label>
        <Input
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
        <label className="text-xs text-text-muted">{t("article.fields.abcClass")}</label>
        <Select
          value={articleForm.abc_class_code ?? "__none__"}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, abc_class_code: v === "__none__" ? null : v }))
          }
        >
          <SelectTrigger>
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
        <label className="text-xs text-text-muted">{t("article.fields.xyzClass")}</label>
        <Select
          value={articleForm.xyz_class_code ?? "__none__"}
          onValueChange={(v) =>
            setArticleForm((s) => ({ ...s, xyz_class_code: v === "__none__" ? null : v }))
          }
        >
          <SelectTrigger>
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
      {/* ABC/XYZ calculation buttons */}
      <div className="col-span-full flex flex-wrap gap-2">
        <Button type="button" variant="outline" size="sm" onClick={() => void handleCalculateAbc()}>
          {t("article.actions.calculateAbc")}
        </Button>
        <Button type="button" variant="outline" size="sm" onClick={() => void handleCalculateXyz()}>
          {t("article.actions.calculateXyz")}
        </Button>
      </div>

      {/* ── CRITICAL SPARE (Phase 2) ── */}
      <SectionTitle>{t("article.sections.criticalSpare")}</SectionTitle>
      <div className="col-span-full flex items-center gap-2">
        <input
          id="is-critical-spare"
          type="checkbox"
          checked={articleForm.is_critical_spare ?? false}
          onChange={(e) =>
            setArticleForm((s) => ({ ...s, is_critical_spare: e.target.checked }))
          }
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
      </div>
      {(articleForm.requires_expiration ?? false) ? (
        <div className="space-y-1">
          <label className="text-xs text-text-muted">{t("article.fields.shelfLifeDays")}</label>
          <Input
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
