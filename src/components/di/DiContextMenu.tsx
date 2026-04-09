/**
 * DiContextMenu.tsx
 *
 * Right-click context menu for DI rows / cards / calendar chips.
 * Uses DropdownMenu positioned at mouse coordinates.
 * Permission-gated items hidden for unauthorized users.
 * Phase 2 – Sub-phase 04 – File 04 – Sprint S4.
 */

import { CheckCircle, Eye, Pencil, RotateCcw, Trash2, XCircle } from "lucide-react";
import { useTranslation } from "react-i18next";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { usePermissions } from "@/hooks/use-permissions";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Statuses for review actions ───────────────────────────────────────────────

const REVIEW_STATES = new Set([
  "pending_review",
  "returned_for_clarification",
  "screened",
  "awaiting_approval",
]);

const EDITABLE_STATES = new Set(["submitted", "returned_for_clarification"]);

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiContextMenuProps {
  di: InterventionRequest;
  /** The trigger element (card / row / chip) */
  children: React.ReactNode;
  /** Forwarded actions */
  onViewDetail: (di: InterventionRequest) => void;
  onEdit?: (di: InterventionRequest) => void;
  onDelete?: (di: InterventionRequest) => void;
  onApprove?: (di: InterventionRequest) => void;
  onReject?: (di: InterventionRequest) => void;
  onReturn?: (di: InterventionRequest) => void;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiContextMenu({
  di,
  children,
  onViewDetail,
  onEdit,
  onDelete,
  onApprove,
  onReject,
  onReturn,
}: DiContextMenuProps) {
  const { t } = useTranslation("di");
  const { can } = usePermissions();

  const canEdit = can("di.create") && EDITABLE_STATES.has(di.status);
  const canDelete = can("di.admin") && di.status === "submitted";
  const canApprove = can("di.approve") && di.status === "awaiting_approval";
  const canReject = can("di.review") && REVIEW_STATES.has(di.status);
  const canReturn = can("di.review") && di.status === "pending_review";

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent className="w-48">
        {/* Always: View detail */}
        <DropdownMenuItem onClick={() => onViewDetail(di)} className="gap-2">
          <Eye className="h-3.5 w-3.5" />
          {t("contextMenu.viewDetail")}
        </DropdownMenuItem>

        {/* Edit */}
        {canEdit && onEdit && (
          <DropdownMenuItem onClick={() => onEdit(di)} className="gap-2">
            <Pencil className="h-3.5 w-3.5" />
            {t("contextMenu.edit")}
          </DropdownMenuItem>
        )}

        {(canApprove || canReject || canReturn) && <DropdownMenuSeparator />}

        {/* Approve */}
        {canApprove && onApprove && (
          <DropdownMenuItem onClick={() => onApprove(di)} className="gap-2">
            <CheckCircle className="h-3.5 w-3.5 text-green-600" />
            {t("contextMenu.approve")}
          </DropdownMenuItem>
        )}

        {/* Reject */}
        {canReject && onReject && (
          <DropdownMenuItem onClick={() => onReject(di)} className="gap-2">
            <XCircle className="h-3.5 w-3.5 text-red-600" />
            {t("contextMenu.reject")}
          </DropdownMenuItem>
        )}

        {/* Return */}
        {canReturn && onReturn && (
          <DropdownMenuItem onClick={() => onReturn(di)} className="gap-2">
            <RotateCcw className="h-3.5 w-3.5 text-orange-600" />
            {t("contextMenu.return")}
          </DropdownMenuItem>
        )}

        {/* Delete */}
        {canDelete && onDelete && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={() => onDelete(di)}
              className="gap-2 text-status-danger focus:text-status-danger"
            >
              <Trash2 className="h-3.5 w-3.5" />
              {t("contextMenu.delete")}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
