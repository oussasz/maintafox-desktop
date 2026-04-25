/**
 * Analytics contract registry IPC (gap 06 sprint 03).
 */

import { invoke } from "@/lib/ipc-invoke";
import type {
  AnalyticsContractVersionRow,
  RegisterAnalyticsContractVersionInput,
} from "@shared/ipc-types";

export async function listAnalyticsContractVersions(): Promise<AnalyticsContractVersionRow[]> {
  return invoke<AnalyticsContractVersionRow[]>("list_analytics_contract_versions");
}

export async function registerAnalyticsContractVersion(
  input: RegisterAnalyticsContractVersionInput,
): Promise<AnalyticsContractVersionRow> {
  return invoke<AnalyticsContractVersionRow>("register_analytics_contract_version", { input });
}
