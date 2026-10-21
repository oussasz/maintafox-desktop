/**
 * Labels of the OT types and of their EN 13306 maintenance nature, read from the SYSTEM
 * reference lists `WORK.ORDER_TYPES` / `WORK.MAINTENANCE_NATURES` (`list_work_order_types`).
 * The frontend keeps no list of its own.
 */

import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";

import { useWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import type { WorkOrderTypeOption } from "@shared/ipc-types";

function isFrench(language: string): boolean {
  return language.toLowerCase().startsWith("fr");
}

export function woTypeLabel(type: WorkOrderTypeOption, language: string): string {
  return isFrench(language) ? type.label : type.label_en;
}

export function woNatureLabel(type: WorkOrderTypeOption, language: string): string | null {
  return isFrench(language) ? type.nature_label : (type.nature_label_en ?? type.nature_label);
}

export function useWoTypeLabels() {
  const { i18n } = useTranslation();
  const types = useWorkOrderTypesCatalog((s) => s.types);
  const load = useWorkOrderTypesCatalog((s) => s.load);

  useEffect(() => {
    // Once per mount when nobody has loaded the catalog yet (`load` is a stable store action).
    const { types: loaded, loading } = useWorkOrderTypesCatalog.getState();
    if (loaded.length === 0 && !loading) void load();
  }, [load]);

  /** Type label in the UI language; `fallback` (stored label) while the catalog loads. */
  const typeLabel = useCallback(
    (code: string | null | undefined, fallback?: string | null): string => {
      const type = code ? types.find((t) => t.code === code) : undefined;
      return type ? woTypeLabel(type, i18n.language) : (fallback ?? code ?? "");
    },
    [types, i18n.language],
  );

  /** Nature label (EN 13306) of the type `code`, null when unknown or not classified. */
  const natureLabel = useCallback(
    (code: string | null | undefined): string | null => {
      const type = code ? types.find((t) => t.code === code) : undefined;
      return type ? woNatureLabel(type, i18n.language) : null;
    },
    [types, i18n.language],
  );

  return { types, typeLabel, natureLabel, language: i18n.language };
}
