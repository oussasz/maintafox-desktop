import { z } from "zod";

import { ASSET_FIELD_REQUIRED_KEY } from "@/schemas/asset-create.schema";

export const assetEditSchema = z.object({
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
  rams_utilization_factor: z.number().min(0.01).max(1.5).default(1.0),
  org_node_id: z.number({ required_error: ASSET_FIELD_REQUIRED_KEY }),
  commissioned_at: z.string().nullable().default(null),
  decommissioned_at: z.string().nullable().default(null),
  description: z.string().max(2000).nullable().default(null),
});

export type AssetEditFormValues = z.infer<typeof assetEditSchema>;
