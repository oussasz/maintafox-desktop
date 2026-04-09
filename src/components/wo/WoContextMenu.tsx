/**
 * WoContextMenu.tsx
 *
 * Right-click / dropdown context menu for WO rows / cards / calendar chips.
 * Uses DropdownMenu, same pattern as DiContextMenu.
 * Permission-gated items hidden for unauthorized users.
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
import { usePermissions } from "@/hooks/use-permissions";
import type { WorkOrder } from "@shared/ipc-types";

// ── Status sets ─────────────────────────────────────────────────────────────

const EDITABLE_STATES = new Set(["draft", "planned"]);
const STARTABLE_STATES = new Set(["released", "assigned"]);
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

export function WoContextMenu({
  wo,
  children,
  onViewDetail,
  onEdit,
  onStart,
  onComplete,
  onPrint,
  onDuplicate,
  onCancel,
}: WoContextMenuProps) {
  const { t } = useTranslation("ot");
  const { can } = usePermissions();

  const canEdit = can("ot.edit") && EDITABLE_STATES.has(wo.status);
  const canStart = can("ot.execute") && STARTABLE_STATES.has(wo.status);
  const canComplete = can("ot.execute") && COMPLETABLE_STATES.has(wo.status);
  const canCancel = can("ot.close") && !CANCELLABLE_DENY.has(wo.status);

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent className="w-48">
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
            <Play className="h-3.5 w-3.5 text-green-600" />
            {t("contextMenu.start")}
          </DropdownMenuItem>
        )}

        {/* Complete */}
        {canComplete && onComplete && (
          <DropdownMenuItem onClick={() => onComplete(wo)} className="gap-2">
            <CheckCircle className="h-3.5 w-3.5 text-amber-600" />
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
              className="gap-2 text-status-danger focus:text-status-danger"
            >
              <XCircle className="h-3.5 w-3.5" />
              {t("contextMenu.cancel")}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
