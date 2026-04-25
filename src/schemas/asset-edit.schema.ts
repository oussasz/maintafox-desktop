import { z } from "zod";

export const assetEditSchema = z.object({
  asset_name: z.string().min(1, "required").max(200),
  class_code: z.string().min(1, "required"),
  family_code: z.string().nullable().default(null),
  subfamily_code: z.string().nullable().default(null),
  criticality_code: z.string().min(1, "required"),
  status_code: z.string().min(1, "required"),
  manufacturer: z.string().nullable().default(null),
  model: z.string().nullable().default(null),
  serial_number: z.string().nullable().default(null),
  maintainable_boundary: z.boolean().default(true),
  org_node_id: z.number({ required_error: "required" }),
  commissioned_at: z.string().nullable().default(null),
  decommissioned_at: z.string().nullable().default(null),
  description: z.string().max(2000).nullable().default(null),
});

export type AssetEditFormValues = z.infer<typeof assetEditSchema>;
