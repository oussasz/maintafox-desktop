import { z } from "zod";

/** i18n key under the `equipment` namespace — resolve with `t(message)` when displaying. */
export const ASSET_FIELD_REQUIRED_KEY = "validation.fieldRequired" as const;

export const assetCreateSchema = z.object({
  asset_code: z.string().min(1, ASSET_FIELD_REQUIRED_KEY).max(50),
  asset_name: z.string().min(1, ASSET_FIELD_REQUIRED_KEY).max(200),
  class_code: z.string().min(1, ASSET_FIELD_REQUIRED_KEY),
  family_code: z.string().nullable().default(null),
  subfamily_code: z.string().nullable().default(null),
  criticality_code: z.string().min(1, ASSET_FIELD_REQUIRED_KEY),
  status_code: z.string().min(1, ASSET_FIELD_REQUIRED_KEY),
  manufacturer: z.string().nullable().default(null),
  model: z.string().nullable().default(null),
  serial_number: z.string().nullable().default(null),
  maintainable_boundary: z.boolean().default(true),
  rams_schedule_reference_value_id: z.number().nullable().default(null),
  rams_utilization_factor: z
    .number({ invalid_type_error: "createForm.validation.utilizationRange" })
    .min(0.01, "createForm.validation.utilizationRange")
    .max(1.5, "createForm.validation.utilizationRange")
    .default(1.0),
  org_node_id: z.number({ required_error: ASSET_FIELD_REQUIRED_KEY }),
  parent_asset_id: z.number().nullable().default(null),
  commissioned_at: z.string().nullable().default(null),
});

export type AssetCreateFormValues = z.infer<typeof assetCreateSchema>;
