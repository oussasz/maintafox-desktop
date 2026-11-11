/**
 * Why the Créer / Enregistrer button of the equipment forms is grey: the i18n key (namespace
 * `equipment`) of the sentence to show beside it, or null when the button is available.
 */
export interface AssetFormReasonInput {
  /** The record is being saved. */
  saving: boolean;
  /** Photos are being sent. */
  photosBusy: boolean;
  /** The reference lists (class, criticality, status) have been loaded. */
  catalogReady: boolean;
  /** Loading the reference lists failed. */
  catalogFailed: boolean;
}

export type AssetFormReasonKey =
  | "actionReason.saving"
  | "actionReason.photosBusy"
  | "actionReason.catalogFailed"
  | "actionReason.catalogLoading";

export function assetFormDisabledReasonKey(input: AssetFormReasonInput): AssetFormReasonKey | null {
  if (input.saving) return "actionReason.saving";
  if (input.photosBusy) return "actionReason.photosBusy";
  if (!input.catalogReady) {
    return input.catalogFailed ? "actionReason.catalogFailed" : "actionReason.catalogLoading";
  }
  return null;
}
