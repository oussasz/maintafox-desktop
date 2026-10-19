/**
 * Shipped permit types show their name in the user's language while the stored name is still the
 * one the app seeded (`m20260615_000067_permit_domain_core`: LOTO, Hot work, Confined space, Cold
 * work, Electrical LV). The type is recognised by its code, never by the name text; a name the
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
