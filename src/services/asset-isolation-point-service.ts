// ADR-003: invoke() calls for the isolation points kept on an equipment (L4-12).

import { z } from "zod";

import { invoke } from "@/lib/ipc-invoke";
import { type IsolationOptions, IsolationOptionsSchema } from "@/services/permit-service";

import {
  type EquipmentIsolationPoint,
  EquipmentIsolationPointSchema,
} from "./permit-safety-service";

export interface EquipmentIsolationPointUpsertInput {
  /** null: a new point. */
  id: number | null;
  equipment_id: number;
  /** Required to edit a point. */
  expected_row_version: number | null;
  point_code: string;
  label: string;
  /** Active `PERMIT.ENERGY_TYPES` value. */
  energy_type_id: number;
  /** Active `PERMIT.ISOLATION_METHODS` value (the plant's list). */
  isolation_method_id: number;
  normal_state: string | null;
}

export async function listEquipmentIsolationPoints(
  equipmentId: number,
  includeInactive: boolean,
): Promise<EquipmentIsolationPoint[]> {
  const raw = await invoke<unknown>("list_equipment_isolation_points", {
    equipmentId,
    includeInactive,
  });
  return z.array(EquipmentIsolationPointSchema).parse(raw);
}

/** Energy types and isolation methods (Données de référence), read with the equipment right. */
export async function listEquipmentIsolationPointOptions(): Promise<IsolationOptions> {
  const raw = await invoke<unknown>("list_equipment_isolation_point_options");
  return IsolationOptionsSchema.parse(raw);
}

export async function upsertEquipmentIsolationPoint(
  input: EquipmentIsolationPointUpsertInput,
): Promise<EquipmentIsolationPoint> {
  const raw = await invoke<unknown>("upsert_equipment_isolation_point", { input });
  return EquipmentIsolationPointSchema.parse(raw);
}

export async function setEquipmentIsolationPointActive(input: {
  id: number;
  expected_row_version: number;
  is_active: boolean;
}): Promise<EquipmentIsolationPoint> {
  const raw = await invoke<unknown>("set_equipment_isolation_point_active", { input });
  return EquipmentIsolationPointSchema.parse(raw);
}
