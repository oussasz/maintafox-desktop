import {
  BookMarked,
  ClipboardList,
  FileBadge,
  FileText,
  Shield,
  type LucideIcon,
} from "lucide-react";
import { useTranslation } from "react-i18next";
import { NavLink, Navigate, Outlet } from "react-router-dom";

import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useLocaleStore } from "@/stores/locale-store";

import { DOC_CATEGORY_SLUGS, type DocumentationCategorySlug } from "./documentation-slugs";

const ICONS: Record<DocumentationCategorySlug, LucideIcon> = {
  "technical-manuals": FileText,
  sops: ClipboardList,
  "safety-protocols": Shield,
  "compliance-certificates": FileBadge,
};

export function DocumentationModuleLayout() {
  const { t } = useTranslation("documentation");
  const rtl = useLocaleStore((s) => s.direction === "rtl");

  return (
    <div className={cn(mfLayout.moduleRoot, "bg-surface-0")}>
      <header className={mfLayout.moduleHeader}>
        <div className="flex min-w-0 items-center gap-2">
          <BookMarked className={mfLayout.moduleHeaderIcon} aria-hidden />
          <div className="min-w-0">
            <h1 className={mfLayout.moduleTitle}>{t("module.title")}</h1>
            <p className="truncate text-xs text-text-muted">{t("module.subtitle")}</p>
          </div>
        </div>
      </header>

      <div className={cn("flex min-h-0 flex-1", rtl ? "flex-row-reverse" : "flex-row")}>
        <aside className="flex w-60 shrink-0 flex-col border-e border-surface-border bg-surface-1 py-3">
          <nav className="flex shrink-0 flex-col gap-0.5 px-2" aria-label={t("module.navAria")}>
            {DOC_CATEGORY_SLUGS.map((slug) => {
              const Icon = ICONS[slug];
              return (
                <NavLink
                  key={slug}
                  to={`/documentation/${slug}`}
                  className={({ isActive }) =>
                    cn(
                      "flex items-center gap-2 rounded-lg px-3 py-2 text-sm transition-colors",
                      isActive
                        ? "border border-primary/30 bg-surface-2 text-text-primary shadow-sm"
                        : "border border-transparent text-text-muted hover:border-surface-border hover:bg-surface-2/80 hover:text-text-primary",
                    )
                  }
                >
                  <Icon className="h-4 w-4 shrink-0 opacity-90" aria-hidden />
                  <span className="truncate">{t(`categories.${slug}.label`)}</span>
                </NavLink>
              );
            })}
          </nav>
        </aside>

        <main className="min-h-0 min-w-0 flex-1 overflow-auto bg-surface-0">
          <Outlet />
        </main>
      </div>
    </div>
  );
}

/** Used when `/documentation` is visited without a child path. */
export function DocumentationIndexRedirect() {
  return <Navigate to="/documentation/technical-manuals" replace />;
}
