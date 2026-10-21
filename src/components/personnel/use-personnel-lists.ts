/**
 * Personnel lists for a screen: the plant's values (Données de référence) with their display
 * text, exact-code lookup and review flag. Read only with the personnel view right (the server's
 * rule); without it the lists stay empty and `canRead` is false.
 */
import { useCallback, useEffect, useMemo } from "react";
import { useTranslation } from "react-i18next";

import { useCanReadList } from "@/lib/list-access";
import {
  type PersonnelListName,
  findPersonnelOption,
  personnelOptionTexts,
  usePersonnelListsStore,
} from "@/stores/personnel-lists-store";
import type { PersonnelOption } from "@shared/ipc-types";

const LIST_NAMES: PersonnelListName[] = [
  "employment_statuses",
  "employment_types",
  "absence_types",
  "position_categories",
];

export interface PersonnelLists {
  canRead: boolean;
  loading: boolean;
  error: unknown;
  reload: () => Promise<void>;
  /** Values of a list, in the plant's order. */
  values: (list: PersonnelListName) => PersonnelOption[];
  /** The value with exactly this code. */
  option: (list: PersonnelListName, code: string | null | undefined) => PersonnelOption | null;
  /** Display text of a code (name, plus the code when two values look alike); the code itself when unknown. */
  text: (list: PersonnelListName, code: string | null | undefined) => string;
}

export function usePersonnelLists(): PersonnelLists {
  const { i18n } = useTranslation();
  const canRead = useCanReadList("people");
  const options = usePersonnelListsStore((s) => s.options);
  const loading = usePersonnelListsStore((s) => s.loading);
  const error = usePersonnelListsStore((s) => s.error);
  const load = usePersonnelListsStore((s) => s.load);

  useEffect(() => {
    if (canRead) void load();
  }, [canRead, load]);

  const texts = useMemo(() => {
    const out = {} as Record<PersonnelListName, Map<string, string>>;
    for (const list of LIST_NAMES) {
      out[list] = personnelOptionTexts(options?.[list] ?? [], i18n.language);
    }
    return out;
  }, [options, i18n.language]);

  const values = useCallback((list: PersonnelListName) => options?.[list] ?? [], [options]);
  const option = useCallback(
    (list: PersonnelListName, code: string | null | undefined) =>
      findPersonnelOption(options?.[list], code),
    [options],
  );
  const text = useCallback(
    (list: PersonnelListName, code: string | null | undefined) =>
      code == null ? "" : (texts[list].get(code) ?? code),
    [texts],
  );
  const reload = useCallback(() => load(true), [load]);

  return useMemo(
    () => ({ canRead, loading, error, reload, values, option, text }),
    [canRead, loading, error, reload, values, option, text],
  );
}

/**
 * Name and behaviour tag of an absence type, by exact code, for screens that draw absences
 * (planning boards). Null when the code is not in the list or the lists cannot be read.
 */
export function useAbsenceTypeLookup(): (
  code: string,
) => { text: string; tag: string | null } | null {
  const lists = usePersonnelLists();
  return useCallback(
    (code: string) => {
      const value = lists.option("absence_types", code);
      return value ? { text: lists.text("absence_types", code), tag: value.tag } : null;
    },
    [lists],
  );
}
