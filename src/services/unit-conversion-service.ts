import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";

const UnitConversionSchema = z.object({
  id: z.number(),
  entity_sync_id: z.string(),
  from_unit_value_id: z.number(),
  from_unit_code: z.string(),
  from_unit_label: z.string(),
  to_unit_value_id: z.number(),
  to_unit_code: z.string(),
  to_unit_label: z.string(),
  article_id: z.number().nullable(),
  article_code: z.string().nullable(),
  factor: z.number(),
  notes: z.string().nullable(),
  is_active: z.boolean(),
  row_version: z.number(),
  updated_at: z.string(),
});

export type UnitConversion = z.infer<typeof UnitConversionSchema>;

const ConvertedQuantitySchema = z.object({
  quantity: z.number(),
  factor: z.number(),
  path: z.enum(["identity", "direct", "inverse"]),
  conversion_id: z.number().nullable(),
});

export type ConvertedQuantity = z.infer<typeof ConvertedQuantitySchema>;

/** Conversions for every article plus, when `articleId` is given, that article's own. */
export async function listUnitConversions(
  articleId: number | null,
  includeInactive = false,
): Promise<UnitConversion[]> {
  const raw = await invoke<unknown>("list_unit_conversions", { articleId, includeInactive });
  return z.array(UnitConversionSchema).parse(raw);
}

/** `1 from unit = factor × to unit`; the inverse is derived. */
export async function createUnitConversion(input: {
  from_unit_value_id: number;
  to_unit_value_id: number;
  article_id: number | null;
  factor: number;
  notes: string | null;
}): Promise<UnitConversion> {
  const raw = await invoke<unknown>("create_unit_conversion", { input });
  return UnitConversionSchema.parse(raw);
}

export async function updateUnitConversion(input: {
  id: number;
  expected_row_version: number;
  factor: number;
  notes: string | null;
}): Promise<UnitConversion> {
  const raw = await invoke<unknown>("update_unit_conversion", { input });
  return UnitConversionSchema.parse(raw);
}

export async function deactivateUnitConversion(
  id: number,
  expectedRowVersion: number,
): Promise<UnitConversion> {
  const raw = await invoke<unknown>("deactivate_unit_conversion", { id, expectedRowVersion });
  return UnitConversionSchema.parse(raw);
}

export async function convertUnitQuantity(
  quantity: number,
  fromUnitValueId: number,
  toUnitValueId: number,
  articleId: number | null,
): Promise<ConvertedQuantity> {
  const raw = await invoke<unknown>("convert_unit_quantity", {
    quantity,
    fromUnitValueId,
    toUnitValueId,
    articleId,
  });
  return ConvertedQuantitySchema.parse(raw);
}

const StockQuantitySchema = z.object({
  stock_quantity: z.number(),
  stock_unit_value_id: z.number(),
  entered_unit_value_id: z.number().nullable(),
  entered_quantity: z.number(),
  factor: z.number(),
  path: z.string(),
});
export type StockQuantity = z.infer<typeof StockQuantitySchema>;

/**
 * The stock quantity a quantity entered in another unit stands for (receipt and issue screens),
 * with the same rule and rounding as the posting: the article's own conversion, then the plant
 * unit's factor, then a general conversion between standard units.
 */
export async function previewInventoryStockQuantity(
  articleId: number,
  enteredUnitValueId: number | null,
  quantity: number,
): Promise<StockQuantity> {
  const raw = await invoke<unknown>("preview_inventory_stock_quantity", {
    articleId,
    enteredUnitValueId,
    quantity,
  });
  return StockQuantitySchema.parse(raw);
}
