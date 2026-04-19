import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router-dom";

import { usePermissions } from "@/hooks/use-permissions";
import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";

export interface NavItem {
  key: string;
  labelKey: string;
  path: string;
  icon: ReactNode;
  groupKey?: string;
  isGroupHeader?: boolean;
  /** Permission required to see this nav item. If undefined, always visible. */
  requiredPermission?: string;
}

interface SidebarProps {
  items: NavItem[];
}

export function Sidebar({ items }: SidebarProps) {
  const { t } = useTranslation("shell");
  const collapsed = useAppStore((s) => s.sidebarCollapsed);
  const hoverOpen = useAppStore((s) => s.sidebarHoverOpen);
  const setSidebarHoverOpen = useAppStore((s) => s.setSidebarHoverOpen);
  const { can } = usePermissions();
  const location = useLocation();
  const isCompact = collapsed && !hoverOpen;

  // Filter non-header items by permission, then group — hide empty groups
  const visibleItems = items.filter(
    (item) => item.isGroupHeader || !item.requiredPermission || can(item.requiredPermission),
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
      onMouseEnter={() => setSidebarHoverOpen(true)}
      onMouseLeave={() => setSidebarHoverOpen(false)}
      onFocusCapture={() => setSidebarHoverOpen(true)}
      onBlurCapture={(event) => {
        const nextFocused = event.relatedTarget;
        if (!nextFocused || !event.currentTarget.contains(nextFocused as Node)) {
          setSidebarHoverOpen(false);
        }
      }}
      className={cn(
        "fixed left-0 top-topbar bottom-statusbar z-20 flex flex-col",
        "border-r border-surface-border bg-surface-1",
        "overflow-y-auto overflow-x-hidden transition-all duration-normal",
        isCompact ? "w-sidebar-sm" : "w-sidebar",
      )}
    >
      <div className="flex flex-col gap-0.5 py-2 px-1.5">
        {groups.map((group, gi) => (
          <div key={group.header?.key ?? `g-${gi}`}>
            {/* Group header: keep vertical slot in compact mode to prevent icon jump */}
            {group.header && (
              <div
                className="px-2 pt-3 pb-1 text-2xs font-semibold uppercase
                           tracking-wider text-text-muted"
              >
                {isCompact ? (
                  <div className="flex items-center" aria-hidden="true">
                    <span className="h-px w-full rounded bg-surface-border/70" />
                  </div>
                ) : (
                  t(group.header.labelKey as never)
                )}
              </div>
            )}
            {/* Nav items */}
            {group.children.map((item) => {
              const tab = new URLSearchParams(location.search).get("tab");
              const isActive =
                item.path === "/"
                  ? location.pathname === "/"
                  : item.key === "training"
                    ? location.pathname === "/personnel" && tab === "training"
                    : item.key === "personnel"
                      ? location.pathname === "/personnel" && tab !== "training"
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
                  title={isCompact ? t(item.labelKey as never) : undefined}
                  className={cn(
                    "flex items-center gap-2.5 rounded-md px-2 py-1.5",
                    "text-sm transition-colors duration-fast",
                    "hover:bg-surface-3",
                    isActive
                      ? "bg-primary-bg/10 text-primary-light font-medium"
                      : "text-text-secondary",
                  )}
                >
                  <span className="h-4 w-4 shrink-0">{item.icon}</span>
                  {!isCompact && <span className="truncate">{t(item.labelKey as never)}</span>}
                </Link>
              );
            })}
          </div>
        ))}
      </div>
    </nav>
  );
}
