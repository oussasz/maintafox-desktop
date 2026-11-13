/**
 * Shipped permit types show their name in the user's language while the stored name is still the
 * one the app seeded (`m20260615_000067_permit_domain_core`: LOTO, Hot work, Confined space, Cold
 * work, Electrical LV; `m20261104_000274_permit_type_work_at_height`: Work at height). The type is recognised by its code, never by the name text; a name the
 * plant typed (or changed) is shown exactly as stored.
 */

import { useCallback } from "react";
import { useTranslation } from "react-i18next";

/** Seeded stored name of each shipped permit type, by code (same text as the Rust seed). */
export const SEEDED_PERMIT_TYPE_NAMES: Readonly<Record<string, string>> = {
  loto: "LOTO",
  hot_work: "Hot work",
  confined_space: "Confined space",
  cold_work: "Cold work",
  electrical_lv: "Electrical LV",
  // Added later (`m20261104_000274_permit_type_work_at_height`, L4-07).
  work_at_height: "Work at height",
};

/** The text key (`permits` namespace) of a shipped type's translated name, or null. */
export function shippedPermitTypeNameKey(
  code: string | null | undefined,
  storedName: string | null | undefined,
): string | null {
  if (!code || storedName == null) return null;
  const seeded = Object.prototype.hasOwnProperty.call(SEEDED_PERMIT_TYPE_NAMES, code)
    ? SEEDED_PERMIT_TYPE_NAMES[code]
    : undefined;
  return seeded !== undefined && storedName.trim() === seeded ? `types.shipped.${code}` : null;
}

/**
 * Whether `text` is the seeded English text of a shipped type (its description was seeded with the
 * same words as its name, "Hot work"): not shown, the translated name already says it.
 */
export function isSeededPermitTypeText(
  code: string | null | undefined,
  text: string | null | undefined,
): boolean {
  if (!code || text == null) return false;
  const seeded = Object.prototype.hasOwnProperty.call(SEEDED_PERMIT_TYPE_NAMES, code)
    ? SEEDED_PERMIT_TYPE_NAMES[code]
    : undefined;
  return seeded !== undefined && text.trim() === seeded;
}

/** Returns a function giving the name to show for a permit type (code + stored name). */
export function usePermitTypeName(): (
  code: string | null | undefined,
  storedName: string | null | undefined,
) => string {
  const { t } = useTranslation("permits");
  return useCallback(
    (code, storedName) => {
      const key = shippedPermitTypeNameKey(code, storedName);
      return key ? t(key) : (storedName ?? "");
    },
    [t],
  );
}
