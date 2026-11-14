/**
 * WoContextMenu.tsx
 *
 * Right-click / dropdown context menu for WO rows / cards / calendar chips.
 * Uses DropdownMenu, same pattern as DiContextMenu.
 * Permission-gated items hidden for unauthorized users: the rights are the user's at the work
 * order's site (`useWoAccess`, loaded when the menu opens). "Démarrer", "Terminer" and "Annuler"
 * are execution actions: someone who does not carry out the work order sees them greyed with the
 * reason, naming who it is assigned to (`useWoExecutionStanding`, L5-08).
 * Phase 2 – Sub-phase 05 – File 04 – Sprint S4.
 */

import { CheckCircle, Copy, Eye, Pencil, Play, Printer, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useWoAccess } from "@/components/wo/WoSiteScope";
import { useWoExecutionStanding } from "@/components/wo/use-wo-execution-standing";
import type { WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// ── Status sets — Option B lifecycle ────────────────────────────────────────

// The backend edits a draft only; a planning WO changes through its planning tab.
const EDITABLE_STATES = new Set(["draft"]);
const STARTABLE_STATES = new Set(["ready"]);
const COMPLETABLE_STATES = new Set(["in_progress"]);
// L1-33: a finished ("Terminé") work order goes to its close, not to a cancellation.
const CANCELLABLE_DENY = new Set(["completed", "closed", "cancelled"]);

// ── Props ───────────────────────────────────────────────────────────────────

interface WoContextMenuProps {
  wo: WorkOrder;
  children: React.ReactNode;
  onViewDetail: (wo: WorkOrder) => void;
  onEdit?: (wo: WorkOrder) => void;
  onStart?: (wo: WorkOrder) => void;
  onComplete?: (wo: WorkOrder) => void;
  onPrint?: (wo: WorkOrder) => void;
  onDuplicate?: (wo: WorkOrder) => void;
  onCancel?: (wo: WorkOrder) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function WoContextMenu({ children, ...items }: WoContextMenuProps) {
  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent className="w-48">
        {/* Mounted only while the menu is open: one rights lookup per opened menu, not per row. */}
        <WoContextMenuItems {...items} />
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function WoContextMenuItems({
  wo,
  onViewDetail,
  onEdit,
  onStart,
  onComplete,
  onPrint,
  onDuplicate,
  onCancel,
}: Omit<WoContextMenuProps, "children">) {
  const { t } = useTranslation("ot");
  const { can } = useWoAccess(wo);
  const execution = useWoExecutionStanding(wo);

  const canEdit = can(P.OT_EDIT) && EDITABLE_STATES.has(wo.status_code ?? "");
  const canStart = can(P.OT_EDIT) && STARTABLE_STATES.has(wo.status_code ?? "");
  const canComplete = can(P.OT_EDIT) && COMPLETABLE_STATES.has(wo.status_code ?? "");
  const canCancel = can(P.OT_EDIT) && !CANCELLABLE_DENY.has(wo.status_code ?? "");

  return (
    <>
      {/* Always: View detail */}
      <DropdownMenuItem onClick={() => onViewDetail(wo)} className="gap-2">
        <Eye className="h-3.5 w-3.5" />
        {t("contextMenu.viewDetail")}
      </DropdownMenuItem>

      {/* Edit */}
      {/* L1-35: a draft is edited by its creator or its execution actors (the cancel rule). */}
      {canEdit && onEdit && (
        <DropdownMenuItem
          onClick={() => {
            if (execution.cancelBlocker === null) onEdit(wo);
          }}
          className="gap-2"
          disabled={execution.cancelBlocker !== null}
          title={execution.cancelBlocker ?? undefined}
        >
          <Pencil className="h-3.5 w-3.5" />
          <ExecutionItemLabel label={t("contextMenu.edit")} blocker={execution.cancelBlocker} />
        </DropdownMenuItem>
      )}

      {(canStart || canComplete) && <DropdownMenuSeparator />}

      {/* Start */}
      {canStart && onStart && (
        <DropdownMenuItem
          onClick={() => {
            if (execution.blocker === null) onStart(wo);
          }}
          className="gap-2"
          disabled={execution.blocker !== null}
          title={execution.blocker ?? undefined}
        >
          <Play className="h-3.5 w-3.5 text-tone-success-fg" />
          <ExecutionItemLabel label={t("contextMenu.start")} blocker={execution.blocker} />
        </DropdownMenuItem>
      )}

      {/* Complete */}
      {canComplete && onComplete && (
        <DropdownMenuItem
          onClick={() => {
            // A greyed item never runs its action (L5-08).
            if (execution.blocker === null) onComplete(wo);
          }}
          className="gap-2"
          disabled={execution.blocker !== null}
          title={execution.blocker ?? undefined}
        >
          <CheckCircle className="h-3.5 w-3.5 text-tone-warning-fg" />
          <ExecutionItemLabel label={t("contextMenu.complete")} blocker={execution.blocker} />
        </DropdownMenuItem>
      )}

      <DropdownMenuSeparator />

      {/* Print */}
      {onPrint && (
        <DropdownMenuItem onClick={() => onPrint(wo)} className="gap-2">
          <Printer className="h-3.5 w-3.5" />
          {t("contextMenu.print")}
        </DropdownMenuItem>
      )}

      {/* Duplicate */}
      {onDuplicate && (
        <DropdownMenuItem onClick={() => onDuplicate(wo)} className="gap-2">
          <Copy className="h-3.5 w-3.5" />
          {t("contextMenu.duplicate")}
        </DropdownMenuItem>
      )}

      {/* Cancel */}
      {canCancel && onCancel && (
        <>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            onClick={() => {
              if (execution.cancelBlocker === null) onCancel(wo);
            }}
            className="gap-2 text-tone-danger-fg focus:text-tone-danger-fg"
            disabled={execution.cancelBlocker !== null}
            title={execution.cancelBlocker ?? undefined}
          >
            <XCircle className="h-3.5 w-3.5" />
            <ExecutionItemLabel label={t("contextMenu.cancel")} blocker={execution.cancelBlocker} />
          </DropdownMenuItem>
        </>
      )}
    </>
  );
}

/** An execution item's label, with the reason it is greyed as visible text under it. */
function ExecutionItemLabel({ label, blocker }: { label: string; blocker: string | null }) {
  if (blocker === null) return <>{label}</>;
  return (
    <span className="flex flex-col">
      <span>{label}</span>
      <span className="text-xs text-text-muted">{blocker}</span>
    </span>
  );
}
