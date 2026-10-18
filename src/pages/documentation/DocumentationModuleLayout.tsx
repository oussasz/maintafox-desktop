/**
 * Document library shell: one entry per document category of the plant's list (Données de
 * référence, `DOCUMENT.CATEGORIES`), in the list's order. A category flagged "à revoir" stays
 * listed (its documents remain reachable) with the review marker.
 */
import { BookMarked, FileText } from "lucide-react";
import { useTranslation } from "react-i18next";
import { NavLink, Navigate, Outlet } from "react-router-dom";

import {
  defaultDocumentCategory,
  documentCategoryPath,
  useDocumentCategories,
} from "@/components/documentation/use-document-categories";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { ReviewMark } from "@/components/ui/review-mark";
import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";
import { useLocaleStore } from "@/stores/locale-store";

export function DocumentationModuleLayout() {
  const { t } = useTranslation("documentation");
  const rtl = useLocaleStore((s) => s.direction === "rtl");
  const categories = useDocumentCategories();

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
            {!categories.canRead ? (
              <ListAccessNote list="documentCategories" className="px-3 py-2" />
            ) : categories.error != null ? (
              <ErrorState
                className="px-3 py-4"
                error={categories.error}
                title={t("module.categoriesLoadError")}
                onRetry={() => void categories.reload()}
              />
            ) : !categories.loaded ? (
              <LoadingState />
            ) : categories.values.length === 0 ? (
              <p className="px-3 py-2 text-xs text-text-muted">{t("module.noCategories")}</p>
            ) : (
              categories.values.map((category) => (
                <NavLink
                  key={category.code}
                  to={documentCategoryPath(category.code)}
                  className={({ isActive }) =>
                    cn(
                      "flex min-h-10 items-center gap-2 rounded-lg px-3 py-2 text-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                      isActive
                        ? "border border-primary/30 bg-surface-2 text-text-primary shadow-sm"
                        : "border border-transparent text-text-muted hover:border-surface-border hover:bg-surface-2/80 hover:text-text-primary",
                    )
                  }
                >
                  <FileText className="h-4 w-4 shrink-0 opacity-90" aria-hidden />
                  <span className="min-w-0 flex-1 truncate">{categories.text(category.code)}</span>
                  {category.review_required ? <ReviewMark /> : null}
                </NavLink>
              ))
            )}
          </nav>
        </aside>

        <main className="min-h-0 min-w-0 flex-1 overflow-auto bg-surface-0">
          <Outlet />
        </main>
      </div>
    </div>
  );
}

/** Used when `/documentation` is visited without a child path: opens the first category. */
export function DocumentationIndexRedirect() {
  const { t } = useTranslation("documentation");
  const categories = useDocumentCategories();
  // Without the right, or while the list fails, the side panel already says why.
  if (!categories.canRead || categories.error != null) return null;
  if (!categories.loaded) return <LoadingState />;
  const first = defaultDocumentCategory(categories.values);
  if (!first) {
    return (
      <EmptyState
        className="m-4"
        title={t("module.noCategories")}
        description={t("module.noCategoriesHint")}
      />
    );
  }
  return <Navigate to={documentCategoryPath(first.code)} replace />;
}
