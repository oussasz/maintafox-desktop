/**
 * Documents & photos tab: the document library entries linked to the asset (manuals, procedures,
 * safety protocols, certificates) with download and upload, and the asset's photos.
 */

import { Download, FileText, Upload } from "lucide-react";
import { useId, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetPhotoGallery } from "@/components/assets/AssetPhotoGallery";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import {
  type DocumentCategories,
  useDocumentCategories,
} from "@/components/documentation/use-document-categories";
import { EntityFormDialog, EntityFormFooter } from "@/components/entity-form";
import { FormField } from "@/components/ui/FormField";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Input } from "@/components/ui/input";
import { ListAccessNote } from "@/components/ui/list-access-note";
import { ReviewMark } from "@/components/ui/review-mark";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { isExportCancelled, saveBlobWithDialog } from "@/export";
import { useFormatters } from "@/hooks/use-formatters";
import { usePermissions } from "@/hooks/use-permissions";
import { describeAppError } from "@/lib/app-error";
import {
  MAX_LIBRARY_DOCUMENT_BYTES,
  fileToNumberArray,
  getLibraryDocumentFile,
  listLibraryDocuments,
  uploadLibraryDocument,
} from "@/services/documentation-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { Asset, LibraryDocument, LibraryDocumentCategory } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function toast(title: string, variant: "success" | "destructive", description?: string | null) {
  pushAppToast({ title, variant, ...(description ? { description } : {}) });
}

export function AssetDocumentsTab({ asset }: { asset: Asset }) {
  const { t } = useTranslation(["equipment", "documentation"]);
  const { formatDate, formatNumber } = useFormatters();
  const { can } = usePermissions();
  const canView = can(P.DOC_VIEW);
  const load = useMemo(
    () => (canView ? () => listLibraryDocuments(null, asset.id) : null),
    [asset.id, canView],
  );
  const { data, error, loading, reload } = useLoad<LibraryDocument[]>(load, String(asset.id));
  const [uploadOpen, setUploadOpen] = useState(false);
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

  return (
    <div className="space-y-4">
      <DetailSectionCard title={t("documents.title")} icon={FileText}>
        {!canView ? (
          <p className="text-sm text-text-muted">{t("documents.noPermission")}</p>
        ) : loading && !data ? (
          <LoadingState />
        ) : error || !data ? (
          <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />
        ) : data.length === 0 ? (
          <p className="text-sm text-text-muted">{t("empty.noDocuments")}</p>
        ) : (
          <ul className="divide-y divide-surface-border">
            {data.map((doc) => (
              <li key={doc.id} className="flex flex-wrap items-center gap-3 py-2">
                <div className="min-w-0 flex-1">
                  <p className="truncate text-sm font-medium">{doc.title}</p>
                  <p className="text-xs text-text-muted">
                    {categories.text(doc.category)}
                    {categories.loaded &&
                    (categories.option(doc.category)?.review_required ?? true) ? (
                      <ReviewMark className="ms-1" />
                    ) : null}{" "}
                    · {doc.fileName} ·{" "}
                    {t("documents.size", { kb: formatNumber(Math.ceil(doc.sizeBytes / 1024)) })} ·{" "}
                    {formatDate(doc.uploadedAt)}
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
        )}
        <PermissionGate permission={P.DOC_MANAGE}>
          <div className="flex justify-end pt-2">
            <Button variant="outline" className="h-11 gap-1.5" onClick={() => setUploadOpen(true)}>
              <Upload className="h-4 w-4" aria-hidden />
              {t("documents.upload")}
            </Button>
          </div>
        </PermissionGate>
      </DetailSectionCard>

      <AssetPhotoGallery
        assetId={asset.id}
        onToast={(msg, variant) =>
          toast(msg, variant === "destructive" ? "destructive" : "success")
        }
      />

      {uploadOpen ? (
        <UploadDialog
          asset={asset}
          categories={categories}
          onClose={() => setUploadOpen(false)}
          onSaved={() => {
            setUploadOpen(false);
            void reload();
          }}
        />
      ) : null}
    </div>
  );
}

function UploadDialog({
  asset,
  categories,
  onClose,
  onSaved,
}: {
  asset: Asset;
  categories: DocumentCategories;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation(["equipment", "documentation"]);
  const formId = useId();
  // New documents go under a category of the list that is not under review, chosen by the
  // person: nothing is proposed for them.
  const choices = categories.values.filter((c) => !c.review_required);
  const [category, setCategory] = useState<LibraryDocumentCategory>("");
  const [title, setTitle] = useState("");
  const [file, setFile] = useState<File | null>(null);
  const [saving, setSaving] = useState(false);
  const tooBig = file != null && file.size > MAX_LIBRARY_DOCUMENT_BYTES;
  const invalid = !file || tooBig || !title.trim() || !category;
  // Once the person has started filling the form, say why it cannot be sent yet.
  const categoryMissing = !category && (file != null || title.trim() !== "");

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (invalid || !file) return;
    setSaving(true);
    try {
      await uploadLibraryDocument({
        category,
        equipmentId: asset.id,
        title: title.trim(),
        fileName: file.name,
        fileBytes: await fileToNumberArray(file),
        mimeType: file.type || "application/octet-stream",
        notes: null,
      });
      onSaved();
    } catch {
      // Reported by the global IPC toast.
    } finally {
      setSaving(false);
    }
  };

  return (
    <EntityFormDialog
      open
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
      title={t("documents.uploadTitle", { code: asset.asset_code })}
      footer={
        <EntityFormFooter
          cancelLabel={t("commercial.cancel")}
          primaryLabel={t("documents.upload")}
          onCancel={onClose}
          formId={formId}
          primaryDisabled={invalid}
          primaryLoading={saving}
        />
      }
    >
      <form id={formId} onSubmit={(e) => void submit(e)} className="grid gap-4">
        <FormField
          name={`${formId}-cat`}
          label={t("documents.category")}
          required
          error={categoryMissing ? t("documents.categoryRequired") : undefined}
        >
          {!categories.canRead ? (
            <ListAccessNote list="documentCategories" />
          ) : categories.error != null ? (
            <ErrorState
              className="py-3"
              error={categories.error}
              title={t("documentation:module.categoriesLoadError")}
              onRetry={() => void categories.reload()}
            />
          ) : categories.loaded && choices.length === 0 ? (
            <p className="text-sm text-text-muted">{t("documentation:module.noCategoriesHint")}</p>
          ) : (
            <Select value={category} onValueChange={setCategory} disabled={!categories.loaded}>
              <SelectTrigger id={`${formId}-cat`} className="h-10">
                <SelectValue placeholder={t("documents.categoryPlaceholder")} />
              </SelectTrigger>
              <SelectContent>
                {choices.map((c) => (
                  <SelectItem key={c.code} value={c.code}>
                    {categories.text(c.code)}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
        </FormField>
        <FormField
          name={`${formId}-file`}
          label={t("documents.file")}
          required
          error={
            tooBig
              ? t("documents.tooBig", { mb: Math.round(MAX_LIBRARY_DOCUMENT_BYTES / 1024 / 1024) })
              : undefined
          }
        >
          <Input
            id={`${formId}-file`}
            type="file"
            onChange={(e) => {
              const next = e.target.files?.[0] ?? null;
              setFile(next);
              if (next && !title.trim()) setTitle(next.name.replace(/\.[^.]+$/, ""));
            }}
          />
        </FormField>
        <FormField name={`${formId}-title`} label={t("documents.docTitle")} required>
          <Input id={`${formId}-title`} value={title} onChange={(e) => setTitle(e.target.value)} />
        </FormField>
      </form>
    </EntityFormDialog>
  );
}
