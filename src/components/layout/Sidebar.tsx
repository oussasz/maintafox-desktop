import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link, useLocation } from "react-router-dom";

import { cn } from "@/lib/utils";
import { useAppStore } from "@/store/app-store";

export interface NavItem {
  key: string;
  labelKey: string;
  path: string;
  icon: ReactNode;
  groupKey?: string;
  isGroupHeader?: boolean;
}

interface SidebarProps {
  items: NavItem[];
}

export function Sidebar({ items }: SidebarProps) {
  const { t } = useTranslation("shell");
  const collapsed = useAppStore((s) => s.sidebarCollapsed);
  const location = useLocation();

  type Group = { header: NavItem | null; children: NavItem[] };
  const groups = items.reduce<Group[]>((acc, item) => {
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
  }, []);

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
                {t(group.header.labelKey)}
              </div>
            )}
            {/* Nav items */}
            {group.children.map((item) => {
              const isActive =
                item.path === "/"
                  ? location.pathname === "/"
                  : location.pathname.startsWith(item.path);
              return (
                <Link
                  key={item.key}
                  to={item.path}
                  title={collapsed ? t(item.labelKey) : undefined}
                  className={cn(
                    "flex items-center gap-2.5 rounded-md px-2 py-1.5",
                    "text-sm transition-colors duration-fast",
                    "hover:bg-surface-3",
                    isActive
                      ? "bg-primary-bg/10 text-primary-light font-medium"
                      : "text-text-secondary",
                    collapsed && "justify-center",
                  )}
                >
                  <span className="h-4 w-4 shrink-0">{item.icon}</span>
                  {!collapsed && <span className="truncate">{t(item.labelKey)}</span>}
                </Link>
              );
            })}
          </div>
        ))}
      </div>
    </nav>
  );
}
