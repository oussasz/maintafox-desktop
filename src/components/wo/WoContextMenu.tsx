/**
 * WoContextMenu.tsx
 *
 * Right-click / dropdown context menu for WO rows / cards / calendar chips.
 * Uses DropdownMenu, same pattern as DiContextMenu.
 * Permission-gated items hidden for unauthorized users: the rights are the user's at the work
 * order's site (`useWoAccess`, loaded when the menu opens).
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
import type { WorkOrder } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

// ── Status sets — Option B lifecycle ────────────────────────────────────────

// The backend edits a draft only; a planning WO changes through its planning tab.
const EDITABLE_STATES = new Set(["draft"]);
const STARTABLE_STATES = new Set(["ready"]);
const COMPLETABLE_STATES = new Set(["in_progress"]);
const CANCELLABLE_DENY = new Set(["closed", "cancelled"]);

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
      {canEdit && onEdit && (
        <DropdownMenuItem onClick={() => onEdit(wo)} className="gap-2">
          <Pencil className="h-3.5 w-3.5" />
          {t("contextMenu.edit")}
        </DropdownMenuItem>
      )}

      {(canStart || canComplete) && <DropdownMenuSeparator />}

      {/* Start */}
      {canStart && onStart && (
        <DropdownMenuItem onClick={() => onStart(wo)} className="gap-2">
          <Play className="h-3.5 w-3.5 text-tone-success-fg" />
          {t("contextMenu.start")}
        </DropdownMenuItem>
      )}

      {/* Complete */}
      {canComplete && onComplete && (
        <DropdownMenuItem onClick={() => onComplete(wo)} className="gap-2">
          <CheckCircle className="h-3.5 w-3.5 text-tone-warning-fg" />
          {t("contextMenu.complete")}
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
            onClick={() => onCancel(wo)}
            className="gap-2 text-tone-danger-fg focus:text-tone-danger-fg"
          >
            <XCircle className="h-3.5 w-3.5" />
            {t("contextMenu.cancel")}
          </DropdownMenuItem>
        </>
      )}
    </>
  );
}
