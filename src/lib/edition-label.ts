import { i18n } from "@/i18n";
import { parseEditionId } from "@/lib/edition-capability-catalog";

/**
 * Translated name of a licence edition ("core" -> "Essentiel"). An unknown or custom edition code
 * is shown as issued, since there is no translation to give it. Call it during render from a
 * component that already uses `useTranslation`, so it re-renders on a language change.
 */
export function editionLabel(raw: string | null | undefined): string | null {
  const trimmed = raw?.trim();
  if (!trimmed) return null;
  const id = parseEditionId(trimmed);
  return id ? i18n.t(`auth:login.licenseDetails.editions.${id}`) : trimmed;
}
