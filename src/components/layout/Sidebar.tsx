import type { MouseEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { useModuleCapabilities } from "@/hooks/use-module-capabilities";
import { usePermissions } from "@/hooks/use-permissions";
import { isModuleCapabilityAllowed } from "@/lib/module-capability";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";
import type { PermissionName } from "@shared/rbac/permissions.generated";

export interface NavItem {
  key: string;
  labelKey: string;
  path: string;
  icon: ReactNode;
  groupKey?: string;
  isGroupHeader?: boolean;
  /**
   * Permission required to see this nav item. If undefined, always visible.
   * Sidebar only supports a single permission (no anyOf); use the primary
   * route permission and document OR cases at the call site.
   */
  requiredPermission?: PermissionName;
  /**
   * Alternative permissions that also open the route (route uses `anyOf`). The item is shown
   * when the user holds `requiredPermission` or any of these.
   */
  alsoVisibleWith?: PermissionName[];
}

/** Same rule as the route guard: the primary permission or any alternative opens the item. */
export function canSeeNavItem(item: NavItem, can: (p: PermissionName) => boolean): boolean {
  if (!item.requiredPermission) return true;
  return can(item.requiredPermission) || (item.alsoVisibleWith ?? []).some((p) => can(p));
}

interface SidebarProps {
  items: NavItem[];
}

export function Sidebar({ items }: SidebarProps) {
  const { t } = useTranslation("shell");
  const collapsed = useAppStore((s) => s.sidebarCollapsed);
  const { can } = usePermissions();
  const { capabilityMap } = useModuleCapabilities();
  const location = useLocation();
  const navigate = useNavigate();

  // The menu always changes page inside this window. A modifier click or a middle click on a link
  // would otherwise ask the webview for a new window; route it in place instead.
  const openInPlace = (e: MouseEvent<HTMLAnchorElement>, path: string) => {
    // `auxclick` also fires for the right button (context menu): only the middle button counts there.
    const wantsNewWindow =
      e.type === "auxclick" ? e.button === 1 : e.ctrlKey || e.shiftKey || e.metaKey || e.altKey;
    if (wantsNewWindow) {
      e.preventDefault();
      navigate(path);
    }
  };

  // Filter by RBAC + soft module capabilities, then group — hide empty groups
  const visibleItems = items.filter(
    (item) =>
      item.isGroupHeader ||
      (canSeeNavItem(item, can) &&
        isModuleCapabilityAllowed(capabilityMap, item.requiredPermission)),
  );

  type Group = { header: NavItem | null; children: NavItem[] };
  const groups = visibleItems
    .reduce<Group[]>((acc, item) => {
      if (item.isGroupHeader) {
        acc.push({ header: item, children: [] });
      } else {
        const lastGroup = acc[acc.length - 1];
        if (lastGroup) {
          lastGroup.children.push(item);
        } else {
          acc.push({ header: null, children: [item] });
        }
      }
      return acc;
    }, [])
    .filter((group) => group.children.length > 0);

  return (
    <nav
      className={cn(
        "fixed left-0 top-topbar bottom-statusbar z-20 flex flex-col",
        "border-r border-surface-border bg-surface-1",
        "overflow-y-auto overflow-x-hidden transition-all duration-normal",
        collapsed ? "w-sidebar-sm" : "w-sidebar",
      )}
    >
      <div className="flex flex-col gap-0.5 py-2 px-1.5">
        {groups.map((group, gi) => (
          <div key={group.header?.key ?? `g-${gi}`}>
            {/* Group header — hidden when collapsed */}
            {group.header && !collapsed && (
              <div
                className="px-2 pt-3 pb-1 text-2xs font-semibold uppercase
                           tracking-wider text-text-muted"
              >
                {t(group.header.labelKey as never)}
              </div>
            )}
            {/* Nav items */}
            {group.children.map((item) => {
              const isActive =
                item.path === "/"
                  ? location.pathname === "/"
                  : item.key === "documentation"
                    ? location.pathname.startsWith("/documentation")
                    : item.path.includes("?")
                      ? `${location.pathname}${location.search}` === item.path
                      : location.pathname === item.path ||
                        location.pathname.startsWith(`${item.path}/`);
              return (
                <Link
                  key={item.key}
                  to={item.path}
                  target="_self"
                  onClick={(e) => openInPlace(e, item.path)}
                  onAuxClick={(e) => openInPlace(e, item.path)}
                  title={collapsed ? t(item.labelKey as never) : undefined}
                  className={cn(
                    "flex items-center gap-2.5 rounded-md px-2 py-1.5",
                    "text-sm transition-colors duration-fast",
                    "hover:bg-surface-3",
                    isActive
                      ? "bg-primary-bg text-primary font-medium dark:bg-primary/20 dark:text-primary-light"
                      : "text-text-secondary",
                    collapsed && "justify-center",
                  )}
                >
                  <span className="h-4 w-4 shrink-0">{item.icon}</span>
                  {!collapsed && <span className="truncate">{t(item.labelKey as never)}</span>}
                </Link>
              );
            })}
          </div>
        ))}
      </div>
    </nav>
  );
}
