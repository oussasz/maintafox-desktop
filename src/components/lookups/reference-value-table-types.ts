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

export interface ReferenceTableConfirmState {
  open: boolean;
  title: string;
  description: string;
  confirmLabel: string;
  cancelLabel: string;
  destructive?: boolean;
  busy?: boolean;
  hint?: string | null;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}
