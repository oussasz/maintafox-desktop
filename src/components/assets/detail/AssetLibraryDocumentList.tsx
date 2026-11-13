/**
 * The document library entries linked to one asset (manuals, procedures, safety protocols,
 * certificates), each with a download button. Shared by the asset's Documents tab and the work
 * order sheet, so the technician sees the machine's documents from the order (L7-16).
 */

import { Download } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { useLoad } from "@/components/assets/detail/use-load";
import { useDocumentCategories } from "@/components/documentation/use-document-categories";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { ReviewMark } from "@/components/ui/review-mark";
import { isExportCancelled, saveBlobWithDialog } from "@/export";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import { getLibraryDocumentFile, listLibraryDocuments } from "@/services/documentation-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { LibraryDocument } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function toast(title: string, variant: "success" | "destructive", description?: string | null) {
  pushAppToast({ title, variant, ...(description ? { description } : {}) });
}

export function AssetLibraryDocumentList({
  assetId,
  reloadToken = 0,
}: {
  assetId: number;
  /** Bump to load the list again (after an upload). */
  reloadToken?: number;
}) {
  const { t } = useTranslation(["equipment", "documentation"]);
  const { formatDate, formatBytes } = useFormatters();
  const { can } = usePermissions();
  const canView = can(P.DOC_VIEW);
  const load = useMemo(
    () => (canView ? () => listLibraryDocuments(null, assetId) : null),
    [assetId, canView],
  );
  const { data, error, loading, reload } = useLoad<LibraryDocument[]>(
    load,
    `${assetId}:${reloadToken}`,
  );
  // Document categories come from the plant's list (Données de référence), by exact code.
  const categories = useDocumentCategories();

  const download = async (doc: LibraryDocument) => {
    try {
      const bytes = await getLibraryDocumentFile(doc.id);
      const copy = new Uint8Array(bytes.byteLength);
      copy.set(bytes);
      await saveBlobWithDialog(
        new Blob([copy], { type: doc.mimeType || "application/octet-stream" }),
        doc.fileName,
      );
      toast(t("documents.saved", { name: doc.fileName }), "success");
    } catch (err) {
      if (isExportCancelled(err)) return;
      const { title, description } = describeAppError(err);
      toast(title, "destructive", description);
    }
  };

  if (!canView) return <p className="text-sm text-text-muted">{t("documents.noPermission")}</p>;
  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  if (data.length === 0) return <p className="text-sm text-text-muted">{t("empty.noDocuments")}</p>;

  return (
    <ul className="divide-y divide-surface-border">
      {data.map((doc) => (
        <li key={doc.id} className="flex flex-wrap items-center gap-3 py-2">
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">{doc.title}</p>
            <p className="text-xs text-text-muted">
              {categories.text(doc.category)}
              {categories.loaded && (categories.option(doc.category)?.review_required ?? true) ? (
                <ReviewMark className="ms-1" />
              ) : null}{" "}
              · {doc.fileName} · {formatBytes(doc.sizeBytes)} · {formatDate(doc.uploadedAt)}
            </p>
          </div>
          <Button
            variant="outline"
            className="h-9 gap-1.5"
            aria-label={t("documents.downloadNamed", { name: doc.title })}
            onClick={() => void download(doc)}
          >
            <Download className="h-4 w-4" aria-hidden />
            {t("documents.download")}
          </Button>
        </li>
      ))}
    </ul>
  );
}
