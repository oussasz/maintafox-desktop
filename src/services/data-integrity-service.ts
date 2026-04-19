/**
 * Data integrity workbench IPC (gap 06 sprint 02).
 */

import { invoke } from "@/lib/ipc-invoke";
import type {
  ApplyDataIntegrityRepairInput,
  DataIntegrityFindingRow,
  WaiveDataIntegrityFindingInput,
} from "@shared/ipc-types";

export async function listDataIntegrityFindings(
  limit?: number,
): Promise<DataIntegrityFindingRow[]> {
  return invoke<DataIntegrityFindingRow[]>("list_data_integrity_findings", { limit });
}

export async function runDataIntegrityDetectors(): Promise<number> {
  return invoke<number>("run_data_integrity_detectors_cmd");
}

export async function waiveDataIntegrityFinding(
  input: WaiveDataIntegrityFindingInput,
): Promise<DataIntegrityFindingRow> {
  return invoke<DataIntegrityFindingRow>("waive_data_integrity_finding_cmd", { input });
}

export async function applyDataIntegrityRepair(
  input: ApplyDataIntegrityRepairInput,
): Promise<DataIntegrityFindingRow> {
  return invoke<DataIntegrityFindingRow>("apply_data_integrity_repair_cmd", { input });
}
