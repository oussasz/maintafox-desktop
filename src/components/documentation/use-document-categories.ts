/**
 * Document categories for a screen: the plant's list (Données de référence) with display text,
 * exact-code lookup and review flag. Read only with the document view right (the server's rule);
 * without it the list stays empty and `canRead` is false.
 */
import { useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { useCanReadList } from "@/lib/list-access";
import { findListOption, listOptionTexts } from "@/lib/list-option-text";
import { useDocumentCategoriesStore } from "@/stores/document-categories-store";
import type { DocumentCategoryOption } from "@shared/ipc-types";

export interface DocumentCategories {
  canRead: boolean;
  loading: boolean;
  error: unknown;
  reload: () => Promise<void>;
  /** Categories in the plant's order (empty until read). */
  values: DocumentCategoryOption[];
  /** True once the list has been read. */
  loaded: boolean;
  /** The category with exactly this code. */
  option: (code: string | null | undefined) => DocumentCategoryOption | null;
  /** Display text of a code (name, plus the code when two look alike); the code when unknown. */
  text: (code: string | null | undefined) => string;
}

export function useDocumentCategories(): DocumentCategories {
  const { i18n } = useTranslation();
  const canRead = useCanReadList("documentCategories");
  const categories = useDocumentCategoriesStore((s) => s.categories);
  const loading = useDocumentCategoriesStore((s) => s.loading);
  const error = useDocumentCategoriesStore((s) => s.error);
  const load = useDocumentCategoriesStore((s) => s.load);

  useEffect(() => {
    if (canRead) void load();
  }, [canRead, load]);

  const values = useMemo(() => categories ?? [], [categories]);
  const texts = useMemo(() => listOptionTexts(values, i18n.language), [values, i18n.language]);
  const option = useCallback(
    (code: string | null | undefined) => findListOption(values, code),
    [values],
  );
  const text = useCallback(
    (code: string | null | undefined) => (code == null ? "" : (texts.get(code) ?? code)),
    [texts],
  );
  const reload = useCallback(() => load(true), [load]);

  return useMemo(
    () => ({
      canRead,
      loading,
      error,
      reload,
      values,
      loaded: categories != null,
      option,
      text,
    }),
    [canRead, loading, error, reload, values, categories, option, text],
  );
}

/** Path of a category's page in the document library (the category code is the URL segment). */
export function documentCategoryPath(code: string): string {
  return `/documentation/${encodeURIComponent(code)}`;
}

/**
 * The category a URL segment names: its exact code, or, for links made before the categories
 * came from the plant's list, the same code written with dashes ("technical-manuals").
 */
export function categoryFromSlug(
  values: readonly DocumentCategoryOption[],
  slug: string | undefined,
): DocumentCategoryOption | null {
  if (!slug) return null;
  return (
    findListOption(values, slug) ??
    (slug.includes("-") ? findListOption(values, slug.replace(/-/g, "_")) : null)
  );
}

/** The category a library opens on: the first one not under review, else the first one. */
export function defaultDocumentCategory(
  values: readonly DocumentCategoryOption[],
): DocumentCategoryOption | null {
  return values.find((v) => !v.review_required) ?? values[0] ?? null;
}
