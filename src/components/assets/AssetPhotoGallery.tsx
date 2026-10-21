/**
 * AssetPhotoGallery.tsx
 *
 * GAP EQ-05: Photo gallery with thumbnail grid, lightbox, upload, and delete.
 * Uses Tauri plugin-dialog for file picking; images load via IPC (`read_asset_photo_preview`) as data URLs
 * so rendering does not depend on `convertFileSrc` / asset-protocol (WebView2 + dev server edge cases).
 */

import { Camera, ChevronLeft, ChevronRight, Loader2, Trash2, Upload } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { MediaFileStateBadge, useMediaFileStates } from "@/components/sync/MediaFileStatus";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { describeAppError } from "@/lib/app-error";
import {
  deleteAssetPhoto,
  listAssetPhotos,
  readAssetPhotoPreview,
  uploadAssetPhoto,
} from "@/services/asset-lifecycle-service";
import type { AssetPhoto, MediaFileState } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

function photoDataUrl(preview: { mime_type: string; data_base64: string }): string {
  return `data:${preview.mime_type};base64,${preview.data_base64}`;
}

interface AssetPhotoImageProps {
  photo: AssetPhoto;
  variant: "grid" | "lightbox";
  /** Sync state of the file: a photo from another device may not be downloaded yet. */
  fileState?: MediaFileState | undefined;
  onRetried?: () => void;
}

/**
 * Loads bytes from the Rust side (same permission as listing photos) and displays a `data:` URL.
 */
function AssetPhotoImage({ photo, variant, fileState, onRetried }: AssetPhotoImageProps) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setSrc(null);
    setFailed(false);
    void readAssetPhotoPreview(photo.id)
      .then((preview) => {
        const dataUrl = photoDataUrl(preview);
        if (!cancelled) setSrc(dataUrl);
      })
      .catch((err) => {
        if (import.meta.env.DEV) {
          console.error("[AssetPhotoGallery] read_asset_photo_preview failed", {
            photoId: photo.id,
            expectedDiskPath: photo.file_path,
            error: describeAppError(err).title,
          });
        }
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
    // A download that completes changes the file state: load the picture again.
  }, [photo.file_name, photo.file_path, photo.id, fileState?.state]);

  if (failed) {
    return (
      <div className="flex h-full w-full flex-col items-center justify-center gap-1 bg-surface-2 p-2 text-center text-xs text-text-muted">
        <span className="line-clamp-3 break-all">{photo.file_name}</span>
        <MediaFileStateBadge state={fileState} {...(onRetried ? { onRetried } : {})} />
      </div>
    );
  }
  if (!src) {
    return (
      <div className="flex h-full w-full items-center justify-center">
        <Loader2 className="h-6 w-6 animate-spin text-text-muted" />
      </div>
    );
  }
  const imgClass =
    variant === "grid"
      ? "h-full w-full object-cover transition-transform group-hover:scale-105"
      : "max-h-[85vh] max-w-[85vw] rounded-md object-contain";

  return (
    <img
      src={src}
      alt={photo.caption ?? photo.file_name}
      loading={variant === "grid" ? "lazy" : "eager"}
      className={imgClass}
    />
  );
}

interface AssetPhotoGalleryProps {
  assetId: number;
  onToast?: (msg: string, variant?: "default" | "destructive") => void;
}

export function AssetPhotoGallery({ assetId, onToast }: AssetPhotoGalleryProps) {
  const { t } = useTranslation("equipment");

  const [photos, setPhotos] = useState<AssetPhoto[]>([]);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState(false);
  const [lightboxIndex, setLightboxIndex] = useState<number | null>(null);
  const [deleteConfirmId, setDeleteConfirmId] = useState<number | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);

  const loadPhotos = useCallback(async () => {
    setLoading(true);
    setLoadError(null);
    try {
      const data = await listAssetPhotos(assetId);
      setPhotos(data);
    } catch (err) {
      // Said in place of the grid: a failed load is never "no photo yet".
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, [assetId]);

  useEffect(() => {
    void loadPhotos();
  }, [loadPhotos]);

  const { states: fileStates, refresh: refreshFileStates } = useMediaFileStates(
    photos.map((p) => p.relative_path),
  );

  // ── Upload via Tauri dialog ────────────────────────────────────────────

  const handleUpload = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
      });
      if (!selected) return;

      const filePath =
        typeof selected === "string" ? selected : (selected as { path?: string }).path;
      if (!filePath) return;

      setUploading(true);
      const photo = await uploadAssetPhoto({
        asset_id: assetId,
        source_path: filePath,
        caption: null,
      });
      setPhotos((prev) => [photo, ...prev]);
      onToast?.(t("photos.uploaded"));
    } catch (err) {
      onToast?.(describeAppError(err).title, "destructive");
    } finally {
      setUploading(false);
    }
  };

  // ── Delete ──────────────────────────────────────────────────────────────

  /** Runs inside ConfirmDialog: a failure keeps it open (the IPC toast reports it). */
  const confirmDelete = async () => {
    if (deleteConfirmId === null) return;
    const photoId = deleteConfirmId;
    await deleteAssetPhoto(photoId);
    setPhotos((prev) => prev.filter((p) => p.id !== photoId));
    setDeleteConfirmId(null);
    setLightboxIndex(null);
    onToast?.(t("photos.deleted"));
  };

  // ── Lightbox keyboard nav ───────────────────────────────────────────────

  useEffect(() => {
    if (lightboxIndex === null) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") {
        setLightboxIndex((i) => (i !== null && i > 0 ? i - 1 : i));
      } else if (e.key === "ArrowRight") {
        setLightboxIndex((i) => (i !== null && i < photos.length - 1 ? i + 1 : i));
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [lightboxIndex, photos.length]);

  // ── Render ──────────────────────────────────────────────────────────────

  const lightboxPhoto = lightboxIndex !== null ? photos[lightboxIndex] : null;

  return (
    <Card>
      <CardHeader className="flex flex-row items-center justify-between pb-3">
        <div className="flex items-center gap-2">
          <Camera className="h-4 w-4 text-text-muted" />
          <CardTitle className="text-base">
            {t("photos.title")}
            {photos.length > 0 && (
              <span className="ml-1.5 text-xs font-normal text-text-muted">({photos.length})</span>
            )}
          </CardTitle>
        </div>
        <PermissionGate permission={P.EQ_MANAGE}>
          <Button
            variant="outline"
            size="sm"
            disabled={uploading}
            onClick={() => void handleUpload()}
          >
            {uploading ? (
              <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" />
            ) : (
              <Upload className="mr-1.5 h-3.5 w-3.5" />
            )}
            {t("photos.upload")}
          </Button>
        </PermissionGate>
      </CardHeader>

      <CardContent>
        {loading && photos.length === 0 ? (
          <LoadingState className="h-32" />
        ) : loadError != null ? (
          <ErrorState
            error={loadError}
            title={t("photos.loadError")}
            onRetry={() => void loadPhotos()}
          />
        ) : photos.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 py-8 text-text-muted">
            <Camera className="h-8 w-8" />
            <p className="text-sm">{t("photos.empty")}</p>
            <PermissionGate permission={P.EQ_MANAGE}>
              <Button variant="outline" size="sm" onClick={() => void handleUpload()}>
                <Upload className="mr-1.5 h-3.5 w-3.5" />
                {t("photos.uploadFirst")}
              </Button>
            </PermissionGate>
          </div>
        ) : (
          <div className="grid grid-cols-4 gap-2">
            {photos.map((photo, idx) => (
              <button
                key={photo.id}
                type="button"
                className="group relative aspect-square overflow-hidden rounded-md border bg-surface-2 focus:outline-none focus:ring-2 focus:ring-ring"
                aria-label={t("photos.open", { name: photo.caption ?? photo.file_name })}
                onClick={() => setLightboxIndex(idx)}
              >
                <AssetPhotoImage
                  photo={photo}
                  variant="grid"
                  fileState={fileStates.get(photo.relative_path)}
                  onRetried={() => void refreshFileStates()}
                />
              </button>
            ))}
          </div>
        )}
      </CardContent>

      {/* ── Lightbox: shared Dialog (focus trap, Escape, labelled) ───────── */}
      <Dialog open={lightboxPhoto != null} onOpenChange={(o) => !o && setLightboxIndex(null)}>
        <DialogContent className="max-h-[94vh] max-w-[94vw] border-none bg-black/90 p-2 sm:max-w-[94vw]">
          <DialogTitle className="sr-only">{t("photos.lightboxLabel")}</DialogTitle>
          {lightboxPhoto ? (
            <div className="relative flex items-center justify-center">
              <AssetPhotoImage
                photo={lightboxPhoto}
                variant="lightbox"
                fileState={fileStates.get(lightboxPhoto.relative_path)}
              />
              {lightboxIndex !== null && lightboxIndex > 0 ? (
                <button
                  type="button"
                  aria-label={t("photos.previous")}
                  title={t("photos.previous")}
                  className="absolute left-2 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full bg-black/60 text-white hover:bg-black/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  onClick={() => setLightboxIndex(lightboxIndex - 1)}
                >
                  <ChevronLeft className="h-5 w-5" aria-hidden />
                </button>
              ) : null}
              {lightboxIndex !== null && lightboxIndex < photos.length - 1 ? (
                <button
                  type="button"
                  aria-label={t("photos.next")}
                  title={t("photos.next")}
                  className="absolute right-2 top-1/2 flex h-11 w-11 -translate-y-1/2 items-center justify-center rounded-full bg-black/60 text-white hover:bg-black/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  onClick={() => setLightboxIndex(lightboxIndex + 1)}
                >
                  <ChevronRight className="h-5 w-5" aria-hidden />
                </button>
              ) : null}
              <PermissionGate permission={P.EQ_MANAGE}>
                <Button
                  type="button"
                  variant="destructive"
                  className="absolute bottom-3 right-3 h-10 gap-1"
                  onClick={() => setDeleteConfirmId(lightboxPhoto.id)}
                >
                  <Trash2 className="h-4 w-4" aria-hidden />
                  {t("photos.delete")}
                </Button>
              </PermissionGate>
              <div className="absolute bottom-3 left-3 rounded-md bg-black/60 px-2 py-1 text-xs text-white">
                {lightboxPhoto.caption ?? lightboxPhoto.file_name}
              </div>
            </div>
          ) : null}
        </DialogContent>
      </Dialog>

      <ConfirmDialog
        open={deleteConfirmId !== null}
        onOpenChange={(o) => !o && setDeleteConfirmId(null)}
        title={t("photos.deleteConfirmTitle")}
        description={t("photos.deleteConfirmDescription")}
        confirmLabel={t("photos.delete")}
        destructive
        onConfirm={confirmDelete}
      />
    </Card>
  );
}
