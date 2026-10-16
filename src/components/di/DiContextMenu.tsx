/**
 * DiContextMenu.tsx
 *
 * Row action menu of a request (list view). Only the decisions the signed-in user may take on
 * this request are listed (`diActions`); decisions that need a form open their dialog.
 */

import {
  CheckCircle,
  ClipboardCheck,
  Eye,
  Pencil,
  Printer,
  RotateCcw,
  XCircle,
} from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { diActions } from "@/components/di/di-actions";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { usePermissions } from "@/hooks/use-permissions";
import { useSession } from "@/hooks/use-session";
import type { InterventionRequest } from "@shared/ipc-types";

interface DiContextMenuProps {
  di: InterventionRequest;
  /** The trigger element (row button). */
  children: ReactNode;
  onViewDetail: (di: InterventionRequest) => void;
  onEdit: (di: InterventionRequest) => void;
  onTriage: (di: InterventionRequest) => void;
  onApprove: (di: InterventionRequest) => void;
  onReturn: (di: InterventionRequest) => void;
  onClose: (di: InterventionRequest) => void;
  onPrint: (di: InterventionRequest) => void;
}

export function DiContextMenu({
  di,
  children,
  onViewDetail,
  onEdit,
  onTriage,
  onApprove,
  onReturn,
  onClose,
  onPrint,
}: DiContextMenuProps) {
  const { t } = useTranslation("di");
  const { can } = usePermissions();
  const { info } = useSession();
  const actions = diActions(di, info?.user_id, can);
  const decisions = actions.triage || actions.approve || actions.convert || actions.returnForInfo;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-56">
        <DropdownMenuItem onClick={() => onViewDetail(di)} className="gap-2">
          <Eye className="h-3.5 w-3.5" aria-hidden />
          {t("contextMenu.viewDetail")}
        </DropdownMenuItem>
        {actions.edit && (
          <DropdownMenuItem onClick={() => onEdit(di)} className="gap-2">
            <Pencil className="h-3.5 w-3.5" aria-hidden />
            {t("contextMenu.edit")}
          </DropdownMenuItem>
        )}
        <DropdownMenuItem onClick={() => onPrint(di)} className="gap-2">
          <Printer className="h-3.5 w-3.5" aria-hidden />
          {t("action.print")}
        </DropdownMenuItem>

        {decisions && <DropdownMenuSeparator />}
        {actions.triage && (
          <DropdownMenuItem onClick={() => onTriage(di)} className="gap-2">
            <ClipboardCheck className="h-3.5 w-3.5" aria-hidden />
            {t("triage.acceptForReview")}
          </DropdownMenuItem>
        )}
        {(actions.approve || actions.convert) && (
          <DropdownMenuItem onClick={() => onApprove(di)} className="gap-2">
            <CheckCircle className="h-3.5 w-3.5 text-tone-success-fg" aria-hidden />
            {actions.approve ? t("contextMenu.approve") : t("action.convertToWO")}
          </DropdownMenuItem>
        )}
        {actions.returnForInfo && (
          <DropdownMenuItem onClick={() => onReturn(di)} className="gap-2">
            <RotateCcw className="h-3.5 w-3.5" aria-hidden />
            {t("contextMenu.return")}
          </DropdownMenuItem>
        )}
        {actions.close && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => onClose(di)} className="gap-2">
              <XCircle className="h-3.5 w-3.5 text-tone-danger-fg" aria-hidden />
              {t("contextMenu.closeRequest")}
            </DropdownMenuItem>
          </>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
