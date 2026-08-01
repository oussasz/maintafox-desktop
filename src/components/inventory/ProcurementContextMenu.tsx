/**
 * Shared overflow menu for Procurement & Repairables list rows.
 * Declarative actions — reuse for requisitions, POs, repairables, goods receipts, lifecycle.
 */

import { Fragment, type ReactNode } from "react";

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import type { PermissionName } from "@shared/rbac/permissions.generated";

export type ProcurementContextMenuAction = {
  id: string;
  label: string;
  onClick: () => void;
  icon?: ReactNode;
  disabled?: boolean;
  destructive?: boolean;
  /** When set, action is hidden unless the user has this permission. */
  permission?: PermissionName;
  /** Visual separator before this item. */
  separatorBefore?: boolean;
};

export type ProcurementContextMenuProps = {
  children: ReactNode;
  actions: ProcurementContextMenuAction[];
  contentClassName?: string;
};

export function ProcurementContextMenu({
  children,
  actions,
  contentClassName,
}: ProcurementContextMenuProps) {
  const { can } = usePermissions();

  const visible = actions.filter((action) => {
    if (action.permission && !can(action.permission)) return false;
    return true;
  });

  if (visible.length === 0) {
    return <>{children}</>;
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>{children}</DropdownMenuTrigger>
      <DropdownMenuContent className={cn("w-48", contentClassName)} align="end">
        {visible.map((action) => (
          <Fragment key={action.id}>
            {action.separatorBefore ? <DropdownMenuSeparator /> : null}
            <DropdownMenuItem
              disabled={action.disabled ?? false}
              onClick={(event) => {
                event.stopPropagation();
                action.onClick();
              }}
              className={cn(
                "gap-2",
                action.destructive && "text-status-danger focus:text-status-danger",
              )}
            >
              {action.icon}
              {action.label}
            </DropdownMenuItem>
          </Fragment>
        ))}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
