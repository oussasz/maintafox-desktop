/**
 * Frontend-only capability / chrome model for ReferenceValueTable.
 * Not a governance IPC change — adapters map real caps or synthetic rules here.
 */

/** UI capability snapshot used by adapters (not backend governance DTO). */
export interface ReferenceTableCapabilities {
  canCreate: boolean;
  canUpdate: boolean;
  canDeactivateOrDelete: boolean;
  canToggleActive: boolean;
}

/** Rendered with the design-system `ConfirmDialog` (cancel label and busy state are built in). */
export interface ReferenceTableConfirmState {
  open: boolean;
  title: string;
  description: string;
  /** Verb naming the action ("Deactivate value"), never "OK". */
  confirmLabel: string;
  destructive?: boolean;
  hint?: string | null;
  onOpenChange: (open: boolean) => void;
  /** Resolve to close the dialog; reject to keep it open (the global toast reports the failure). */
  onConfirm: () => Promise<void> | void;
}
