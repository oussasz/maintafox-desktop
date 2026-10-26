/**
 * Close-out form of a work order (failure coding, root cause, corrective action, service cost):
 * what the screen shows, what a save stores, and whether a close-out field was changed elsewhere
 * since the form was loaded (AU-18).
 *
 * A save is checked against the work-order version the form was loaded at. That version also
 * moves for changes that are not the close-out (a header transition, an execution entry), so a
 * refused save compares the stored close-out with the one the form started from: equal, the
 * screen is saved again on the new version; different, the change made elsewhere is shown.
 */
import type { WoFailureDetail } from "@/services/wo-closeout-service";
import type { WorkOrder } from "@shared/ipc-types";

export type RepairType = "temporary" | "permanent" | "na";

/** The close-out fields as the screen holds them. */
export interface CloseOutForm {
  symptomId: number | null;
  failureModeId: number | null;
  failureCauseId: number | null;
  failureEffectId: number | null;
  failureMechanismId: number | null;
  detectionMethodId: number | null;
  causeNotDetermined: boolean;
  repairType: RepairType;
  narrative: string;
  rootCauseSummary: string;
  correctiveAction: string;
  serviceCost: string;
}

/** The failure coding as it is stored (one row per work order). */
export interface StoredCoding {
  symptom_id: number | null;
  failure_mode_id: number | null;
  failure_cause_id: number | null;
  failure_effect_id: number | null;
  failure_mechanism_id: number | null;
  detection_method_id: number | null;
  is_temporary_repair: boolean;
  is_permanent_repair: boolean;
  cause_not_determined: boolean;
  notes: string | null;
}

/** The close-out as stored, or as a save stores it (`null`: nothing to store for that field). */
export interface StoredCloseOut {
  coding: StoredCoding;
  rootCause: string | null;
  correctiveAction: string | null;
  serviceCost: number | null;
}

const text = (value: string | null | undefined): string | null => value?.trim() || null;

/** The service cost typed on screen, when it is a number a save stores (zero or more). */
export function parsedServiceCost(value: string): number | null {
  const n = Number.parseFloat(value);
  return Number.isNaN(n) || n < 0 ? null : n;
}

/** The form shown for a work order and its stored failure coding. */
export function formFromStored(wo: WorkOrder, fd: WoFailureDetail | null): CloseOutForm {
  const cost = wo.service_cost_input ?? wo.service_cost;
  return {
    symptomId: fd?.symptom_id ?? null,
    failureModeId: fd?.failure_mode_id ?? null,
    failureCauseId: fd?.failure_cause_id ?? null,
    failureEffectId: fd?.failure_effect_id ?? null,
    failureMechanismId: fd?.failure_mechanism_id ?? null,
    detectionMethodId: fd?.detection_method_id ?? null,
    causeNotDetermined: fd?.cause_not_determined ?? false,
    repairType: fd?.is_temporary_repair
      ? "temporary"
      : fd?.is_permanent_repair
        ? "permanent"
        : "na",
    narrative: fd?.notes ?? "",
    rootCauseSummary: wo.root_cause_summary ?? "",
    correctiveAction: wo.corrective_action_summary ?? "",
    serviceCost: cost != null && cost > 0 ? String(cost) : "",
  };
}

/** The close-out stored for a work order. */
export function storedOf(wo: WorkOrder, fd: WoFailureDetail | null): StoredCloseOut {
  return {
    coding: {
      symptom_id: fd?.symptom_id ?? null,
      failure_mode_id: fd?.failure_mode_id ?? null,
      failure_cause_id: fd?.failure_cause_id ?? null,
      failure_effect_id: fd?.failure_effect_id ?? null,
      failure_mechanism_id: fd?.failure_mechanism_id ?? null,
      detection_method_id: fd?.detection_method_id ?? null,
      is_temporary_repair: fd?.is_temporary_repair ?? false,
      is_permanent_repair: fd?.is_permanent_repair ?? false,
      cause_not_determined: fd?.cause_not_determined ?? false,
      notes: text(fd?.notes),
    },
    rootCause: text(wo.root_cause_summary),
    correctiveAction: text(wo.corrective_action_summary),
    serviceCost: wo.service_cost_input ?? null,
  };
}

/** What a save of the form sends (the coding of an undetermined cause carries no mode / cause). */
export function toSave(form: CloseOutForm): StoredCloseOut {
  const undetermined = form.causeNotDetermined;
  return {
    coding: {
      symptom_id: form.symptomId,
      failure_mode_id: undetermined ? null : form.failureModeId,
      failure_cause_id: undetermined ? null : form.failureCauseId,
      failure_effect_id: undetermined ? null : form.failureEffectId,
      failure_mechanism_id: undetermined ? null : form.failureMechanismId,
      detection_method_id: form.detectionMethodId,
      is_temporary_repair: form.repairType === "temporary",
      is_permanent_repair: form.repairType === "permanent",
      cause_not_determined: undetermined,
      notes: text(form.narrative),
    },
    rootCause: text(form.rootCauseSummary),
    correctiveAction: text(form.correctiveAction),
    serviceCost: parsedServiceCost(form.serviceCost),
  };
}

/** Whether two stored close-outs hold the same values. */
export function sameStored(a: StoredCloseOut, b: StoredCloseOut): boolean {
  const codingKeys = Object.keys(a.coding) as (keyof StoredCoding)[];
  return (
    codingKeys.every((k) => a.coding[k] === b.coding[k]) &&
    a.rootCause === b.rootCause &&
    a.correctiveAction === b.correctiveAction &&
    a.serviceCost === b.serviceCost
  );
}
