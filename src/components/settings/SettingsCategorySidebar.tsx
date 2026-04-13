import { Globe, Monitor, Palette, Search, Shield } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

const CATEGORY_ICONS: Record<string, LucideIcon> = {
  localization: Globe,
  appearance: Palette,
  system: Monitor,
  backup: Shield,
};

interface SettingsCategorySidebarProps {
  categories: string[];
  activeCategory: string | null;
  onSelect: (category: string) => void;
}

export function SettingsCategorySidebar({
  categories,
  activeCategory,
  onSelect,
}: SettingsCategorySidebarProps) {
  const { t } = useTranslation("settings");
  const [search, setSearch] = useState("");

  const filtered = useMemo(() => {
    if (!search.trim()) return categories;
    const q = search.toLowerCase();
    return categories.filter((cat) => {
      const label = String(t(`categories.${cat}` as "categories.localization"));
      return label.toLowerCase().includes(q) || cat.toLowerCase().includes(q);
    });
  }, [categories, search, t]);

  return (
    <nav className="w-56 shrink-0 space-y-2" aria-label={t("page.title") as string}>
      <div className="relative">
        <Search className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-text-muted" />
        <Input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("sidebar.search")}
          className="h-8 pl-8 text-xs"
        />
      </div>
      <div className="space-y-1">
        {filtered.map((cat) => {
          const Icon = CATEGORY_ICONS[cat] ?? Monitor;
          const isActive = cat === activeCategory;
          return (
            <button
              key={cat}
              type="button"
              onClick={() => onSelect(cat)}
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
