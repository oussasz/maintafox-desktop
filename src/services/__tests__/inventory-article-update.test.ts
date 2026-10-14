/**
 * Regression: `update_inventory_article` replaces every item-master column. Editing or
 * deactivating an article must therefore send the stored extended fields (manufacturer,
 * replenishment, ABC/XYZ, critical spare, shelf life …), under the backend's field names.
 */

import { beforeEach, describe, expect, it } from "vitest";

import { mockInvoke } from "@/test/mocks/tauri";

import {
  type InventoryArticleRow,
  articleToInput,
  updateInventoryArticle,
} from "../inventory-service";

function storedArticle(): InventoryArticleRow {
  return {
    id: 7,
    article_code: "ROUL-6204",
    article_name: "Roulement 6204-2RS",
    family_id: 1,
    family_code: "ROUL",
    family_name: "Roulements",
    unit_value_id: 10,
    unit_code: "U",
    unit_label: "Unité",
    criticality_value_id: 20,
    criticality_code: "A",
    criticality_label: "Critique",
    stocking_type_value_id: 11,
    stocking_type_code: "STOCK",
    stocking_type_label: "Stocké",
    tax_category_value_id: 12,
    tax_category_code: "TVA19",
    tax_category_label: "TVA 19 %",
    procurement_category_value_id: null,
    procurement_category_code: null,
    procurement_category_label: null,
    preferred_warehouse_id: 2,
    preferred_warehouse_code: "MAG1",
    preferred_warehouse_name: "Magasin central",
    preferred_location_id: 3,
    preferred_location_code: "A-01",
    preferred_location_name: "Allée A",
    min_stock: 4,
    max_stock: 20,
    reorder_point: 6,
    safety_stock: 2,
    manufacturer_name: "SKF",
    manufacturer_part_number: "6204-2RSH",
    oem_part_number: "P-001-B12",
    replenishment_policy_code: "MIN_MAX",
    economic_order_qty: 12,
    minimum_order_qty: 4,
    maximum_order_qty: 40,
    order_multiple_qty: 2,
    lead_time_days: 21,
    review_period_days: 30,
    abc_class_code: "A",
    xyz_class_code: "X",
    is_critical_spare: 1,
    requires_expiration: 0,
    shelf_life_days: null,
    requires_batch_tracking: 1,
    is_active: 1,
    row_version: 3,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    on_hand_qty: 8,
    reserved_qty: 1,
    available_qty: 7,
    avg_unit_cost: null,
    stock_value: null,
    unvalued_qty: 0,
  };
}

describe("inventory article update payload", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it("articleToInput keeps every stored item-master field", () => {
    const input = articleToInput(storedArticle());
    expect(input).toMatchObject({
      manufacturer_name: "SKF",
      manufacturer_part_number: "6204-2RSH",
      oem_part_number: "P-001-B12",
      replenishment_policy_code: "MIN_MAX",
      economic_order_qty: 12,
      minimum_order_qty: 4,
      maximum_order_qty: 40,
      order_multiple_qty: 2,
      lead_time_days: 21,
      review_period_days: 30,
      abc_class_code: "A",
      xyz_class_code: "X",
      is_critical_spare: true,
      requires_expiration: false,
      requires_batch_tracking: true,
      is_active: true,
    });
  });

  it("deactivation sends the extended fields instead of clearing them", async () => {
    const article = storedArticle();
    mockInvoke.mockResolvedValueOnce({ ...article, is_active: 0, row_version: 4 });

    await updateInventoryArticle(
      article.id,
      article.row_version,
      articleToInput(article, { is_active: false }),
    );

    expect(mockInvoke).toHaveBeenCalledTimes(1);
    const [command, args] = mockInvoke.mock.calls[0] as [
      string,
      { input: Record<string, unknown> },
    ];
    expect(command).toBe("update_inventory_article");
    expect(args.input).toMatchObject({
      is_active: false,
      manufacturer_name: "SKF",
      economic_order_qty: 12,
      lead_time_days: 21,
      abc_class_code: "A",
      is_critical_spare: true,
    });
  });
});
