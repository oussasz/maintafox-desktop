import type { Dispatch, SetStateAction } from "react";

import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ArticleFamily, InventoryArticleInput, LookupValueOption, StockLocation, Warehouse } from "@shared/ipc-types";

export type ArticleEditorFieldsProps = {
  articleForm: InventoryArticleInput;
  setArticleForm: Dispatch<SetStateAction<InventoryArticleInput>>;
  families: ArticleFamily[];
  warehouses: Warehouse[];
  preferredWarehouseLocations: StockLocation[];
  unitOptions: LookupValueOption[];
  criticalityOptions: LookupValueOption[];
  stockingTypeOptions: LookupValueOption[];
  taxCategoryOptions: LookupValueOption[];
  procurementCategoryOptions: LookupValueOption[];
};

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
  return (
    <div className="grid gap-3 md:grid-cols-3">
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
    </div>
  );
}
