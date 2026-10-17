/**
 * Plain-language labels for archive codes (module, class, policies, actions) and the name an
 * archived record is shown under. Unknown codes read "Other …", never the raw code.
 */
import type { TFunction } from "i18next";

import type { ArchiveItemSummary } from "@/services/archive-service";

type T = TFunction<"archive">;

export function archiveModuleLabel(t: T, code: string): string {
  return t(`module.${code}`, { defaultValue: t("module.other") });
}

export function archiveClassLabel(t: T, code: string): string {
  return t(`class.${code}`, { defaultValue: t("class.other") });
}

export function restorePolicyLabel(t: T, code: string): string {
  return t(`restorePolicy.${code}`, { defaultValue: t("restorePolicy.other") });
}

export function purgeModeLabel(t: T, code: string): string {
  return t(`purgeMode.${code}`, { defaultValue: t("purgeMode.other") });
}

export function archiveActionLabel(t: T, code: string): string {
  return t(`action.${code}`, { defaultValue: t("action.other") });
}

/** Business code / title found in the archived payload, if any. */
export function payloadDisplayName(payload: unknown): string | null {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) return null;
  const record = payload as Record<string, unknown>;
  const code = ["code", "reference", "number"]
    .map((k) => record[k])
    .find((v): v is string => typeof v === "string" && v.trim() !== "");
  const title = ["title", "name", "label"]
    .map((k) => record[k])
    .find((v): v is string => typeof v === "string" && v.trim() !== "");
  if (code && title) return `${code} — ${title}`;
  return code ?? title ?? null;
}

/**
 * How an archived record is named on screen: its business code/title from the payload, else
 * the source reference when it is a business code (e.g. "WO-2001"), else "<module> record".
 * A purely numeric reference is a database id and is not shown.
 */
export function archiveRecordName(
  t: T,
  item: Pick<ArchiveItemSummary, "source_module" | "source_record_id">,
  payload?: unknown,
): string {
  const fromPayload = payloadDisplayName(payload);
  if (fromPayload) return fromPayload;
  const ref = item.source_record_id.trim();
  if (ref && !/^\d+$/.test(ref) && !/^[0-9a-f-]{32,36}$/i.test(ref)) return ref;
  return t("record.unnamed", { module: archiveModuleLabel(t, item.source_module) });
}

/** Safe file name part from a record name. */
export function fileSafe(name: string): string {
  return (
    name
      .normalize("NFKD")
      .replace(/[^\w.-]+/g, "-")
      .replace(/-+/g, "-")
      .replace(/^-|-$/g, "")
      .slice(0, 60) || "archive"
  );
}
