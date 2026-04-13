/**
 * AssetTreeContextMenu.tsx
 *
 * Right-click context menu for tree nodes with permission-gated actions.
 * Uses a positioned overlay instead of radix ContextMenu (not installed).
 */

import { AlertTriangle, ClipboardCopy, Move, Pencil, Plus } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";

import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import type { Asset } from "@shared/ipc-types";

interface ContextMenuAction {
  key: string;
  label: string;
  icon: React.ReactNode;
  permission?: string;
  variant?: "default" | "danger";
  onAction: () => void;
}

interface AssetTreeContextMenuProps {
  asset: Asset;
  position: { x: number; y: number };
  onClose: () => void;
  onAddChild: (parent: Asset) => void;
  onEdit: (asset: Asset) => void;
  onMove: (asset: Asset) => void;
  onDecommission: (asset: Asset) => void;
}

export function AssetTreeContextMenu({
  asset,
  position,
  onClose,
  onAddChild,
  onEdit,
  onMove,
  onDecommission,
}: AssetTreeContextMenuProps) {
  const { t } = useTranslation("equipment");
  const { can } = usePermissions();
  const menuRef = useRef<HTMLDivElement>(null);

  // Close on click outside
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  // Close on Escape
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [onClose]);

  const handleCopyCode = useCallback(() => {
    void navigator.clipboard.writeText(asset.asset_code);
    onClose();
  }, [asset.asset_code, onClose]);

  const actions: ContextMenuAction[] = [
    {
      key: "add-child",
      label: t("contextMenu.addChild"),
      icon: <Plus className="h-3.5 w-3.5" />,
      permission: "eq.manage",
      onAction: () => {
        onAddChild(asset);
        onClose();
      },
    },
    {
      key: "edit",
      label: t("contextMenu.edit"),
      icon: <Pencil className="h-3.5 w-3.5" />,
      permission: "eq.manage",
      onAction: () => {
        onEdit(asset);
        onClose();
      },
    },
    {
      key: "copy-code",
      label: t("contextMenu.copyCode"),
      icon: <ClipboardCopy className="h-3.5 w-3.5" />,
      permission: "eq.view",
      onAction: handleCopyCode,
    },
    {
      key: "move",
      label: t("contextMenu.move"),
      icon: <Move className="h-3.5 w-3.5" />,
      permission: "eq.manage",
      onAction: () => {
        onMove(asset);
        onClose();
      },
    },
    {
      key: "decommission",
      label: t("contextMenu.decommission"),
      icon: <AlertTriangle className="h-3.5 w-3.5" />,
      permission: "eq.manage",
      variant: "danger",
      onAction: () => {
        onDecommission(asset);
        onClose();
      },
    },
  ];

  const visibleActions = actions.filter((a) => !a.permission || can(a.permission));

  if (visibleActions.length === 0) return null;

  return (
    <div
      ref={menuRef}
      className="fixed z-50 min-w-[180px] rounded-md border border-surface-border bg-surface p-1 shadow-md"
      style={{ left: position.x, top: position.y }}
      role="menu"
    >
      {visibleActions.map((action, i) => (
        <div key={action.key}>
          {/* Separator before decommission */}
          {action.key === "move" && i > 0 && (
            <div className="my-1 h-px bg-surface-border" role="separator" />
          )}
          <button
            type="button"
            role="menuitem"
            className={cn(
              "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-xs outline-none hover:bg-accent",
              action.variant === "danger" && "text-status-danger",
            )}
            onClick={action.onAction}
          >
            {action.icon}
            {action.label}
          </button>
        </div>
      ))}
    </div>
  );
}
