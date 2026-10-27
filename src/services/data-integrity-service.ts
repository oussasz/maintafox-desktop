/**
 * Data integrity workbench IPC (gap 06 sprint 02).
 */

import { invoke } from "@/lib/ipc-invoke";
import type {
  ApplyDataIntegrityRepairInput,
  DataIntegrityFindingRow,
  IntegrityWaiverApproverOption,
  WaiveDataIntegrityFindingInput,
} from "@shared/ipc-types";

export async function listDataIntegrityFindings(
  limit?: number,
): Promise<DataIntegrityFindingRow[]> {
  return invoke<DataIntegrityFindingRow[]>("list_data_integrity_findings", { limit });
}

/** People the waiver of an error finding may name as second person (the current user excluded). */
export async function listIntegrityWaiverApprovers(): Promise<IntegrityWaiverApproverOption[]> {
  return invoke<IntegrityWaiverApproverOption[]>("list_integrity_waiver_approvers");
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
