import {
  Bell,
  Globe,
  History,
  Link2,
  Monitor,
  Palette,
  Search,
  Shield,
  User,
  Wrench,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { P, type PermissionName } from "@shared/rbac/permissions.generated";

export type SettingsTabId =
  | "profile"
  | "notifications"
  | "general"
  | "security"
  | "backup"
  | "integrations"
  | "activity";

export interface SettingsNavItem {
  id: SettingsTabId;
  icon: LucideIcon;
  /** When true, tab requires `adm.settings` (admin/tenant configuration). */
  requiresAdmSettings?: boolean;
  /** Right the tab needs on its own (the server's read right for what it shows). */
  permission?: PermissionName;
}

export const SETTINGS_TABS: SettingsNavItem[] = [
  { id: "profile", icon: User },
  { id: "notifications", icon: Bell },
  { id: "general", icon: Palette, requiresAdmSettings: true },
  { id: "security", icon: Shield, requiresAdmSettings: true },
  { id: "backup", icon: Wrench, requiresAdmSettings: true },
  { id: "integrations", icon: Link2, requiresAdmSettings: true },
  // Activity journal and audit log: `log.view` (activity_feed), not the settings right.
  { id: "activity", icon: History, permission: P.LOG_VIEW },
];

const LEGACY_CATEGORY_ICONS: Record<string, LucideIcon> = {
  localization: Globe,
  appearance: Palette,
  system: Monitor,
  backup: Shield,
  reliability: Monitor,
};

interface SettingsCategorySidebarProps {
  /** Product tabs (preferred). When provided, `categories` is ignored. */
  tabs?: SettingsNavItem[];
  activeTab?: SettingsTabId | null;
  onSelectTab?: (tab: SettingsTabId) => void;
  /** Legacy DB category mode (kept for compatibility). */
  categories?: string[];
  activeCategory?: string | null;
  onSelect?: (category: string) => void;
}

export function SettingsCategorySidebar({
  tabs,
  activeTab,
  onSelectTab,
  categories,
  activeCategory,
  onSelect,
}: SettingsCategorySidebarProps) {
  const { t } = useTranslation("settings");
  const [search, setSearch] = useState("");

  const usingTabs = Array.isArray(tabs) && tabs.length > 0;

  const filteredTabs = useMemo(() => {
    if (!usingTabs || !tabs) return [];
    if (!search.trim()) return tabs;
    const q = search.toLowerCase();
    return tabs.filter((tab) => {
      const label = String(t(`tabs.${tab.id}` as "tabs.general"));
      return label.toLowerCase().includes(q) || tab.id.toLowerCase().includes(q);
    });
  }, [usingTabs, tabs, search, t]);

  const filteredCategories = useMemo(() => {
    if (usingTabs || !categories) return [];
    if (!search.trim()) return categories;
    const q = search.toLowerCase();
    return categories.filter((cat) => {
      const label = String(t(`categories.${cat}` as "categories.localization"));
      return label.toLowerCase().includes(q) || cat.toLowerCase().includes(q);
    });
  }, [usingTabs, categories, search, t]);

  return (
    <nav className="w-56 shrink-0 space-y-2" aria-label={t("page.title") as string}>
      <div className="relative">
        <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-text-muted" />
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("sidebar.search")}
          className="h-10 pl-8 text-xs"
        />
      </div>
      <div className="space-y-1">
        {usingTabs
          ? filteredTabs.map((tab) => {
              const Icon = tab.icon;
              const isActive = tab.id === activeTab;
              return (
                <button
                  key={tab.id}
                  type="button"
                  onClick={() => onSelectTab?.(tab.id)}
                  className={cn(
                    "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                    isActive
                      ? "bg-primary/10 text-primary"
                      : "text-text-muted hover:bg-muted hover:text-text-primary",
                  )}
                  aria-current={isActive ? "page" : undefined}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  <span>{t(`tabs.${tab.id}` as "tabs.general")}</span>
                </button>
              );
            })
          : filteredCategories.map((cat) => {
              const Icon = LEGACY_CATEGORY_ICONS[cat] ?? Monitor;
              const isActive = cat === activeCategory;
              return (
                <button
                  key={cat}
                  type="button"
                  onClick={() => onSelect?.(cat)}
                  className={cn(
                    "flex w-full items-center gap-3 rounded-md px-3 py-2 text-sm font-medium transition-colors",
                    isActive
                      ? "bg-primary/10 text-primary"
                      : "text-text-muted hover:bg-muted hover:text-text-primary",
                  )}
                  aria-current={isActive ? "page" : undefined}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  <span>{t(`categories.${cat}` as "categories.localization")}</span>
                </button>
              );
            })}
      </div>
    </nav>
  );
}
