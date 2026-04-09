/**
 * AssetPhotoGallery.tsx
 *
 * GAP EQ-05: Photo gallery with thumbnail grid, lightbox, upload, and delete.
 * Uses Tauri plugin-dialog for file picking, convertFileSrc for local images.
 */

import { convertFileSrc } from "@tauri-apps/api/core";
import { Camera, ChevronLeft, ChevronRight, Loader2, Trash2, Upload, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  deleteAssetPhoto,
  listAssetPhotos,
  uploadAssetPhoto,
} from "@/services/asset-lifecycle-service";
import { toErrorMessage } from "@/utils/errors";
import type { AssetPhoto } from "@shared/ipc-types";

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
  const [deleting, setDeleting] = useState(false);

  const loadPhotos = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listAssetPhotos(assetId);
      setPhotos(data);
    } catch {
      setPhotos([]);
    } finally {
      setLoading(false);
    }
  }, [assetId]);

  useEffect(() => {
    void loadPhotos();
  }, [loadPhotos]);

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
      onToast?.(toErrorMessage(err), "destructive");
    } finally {
      setUploading(false);
    }
  };

  // ── Delete ──────────────────────────────────────────────────────────────

  const handleDelete = async (photoId: number) => {
    setDeleting(true);
    try {
      await deleteAssetPhoto(photoId);
      setPhotos((prev) => prev.filter((p) => p.id !== photoId));
      setDeleteConfirmId(null);
      if (lightboxIndex !== null) setLightboxIndex(null);
      onToast?.(t("photos.deleted"));
    } catch (err) {
      onToast?.(toErrorMessage(err), "destructive");
    } finally {
      setDeleting(false);
    }
  };

  // ── Lightbox keyboard nav ───────────────────────────────────────────────

  useEffect(() => {
    if (lightboxIndex === null) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") {
        setLightboxIndex((i) => (i !== null && i > 0 ? i - 1 : i));
      } else if (e.key === "ArrowRight") {
        setLightboxIndex((i) => (i !== null && i < photos.length - 1 ? i + 1 : i));
      } else if (e.key === "Escape") {
        setLightboxIndex(null);
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
        <PermissionGate permission="eq.manage">
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
        {loading ? (
          <div className="flex h-32 items-center justify-center">
            <Loader2 className="h-4 w-4 animate-spin text-text-muted" />
          </div>
        ) : photos.length === 0 ? (
          <div className="flex flex-col items-center justify-center gap-2 py-8 text-text-muted">
            <Camera className="h-8 w-8" />
            <p className="text-sm">{t("photos.empty")}</p>
            <PermissionGate permission="eq.manage">
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
                onClick={() => setLightboxIndex(idx)}
              >
                <img
                  src={convertFileSrc(photo.file_path)}
                  alt={photo.caption ?? photo.file_name}
                  loading="lazy"
                  className="h-full w-full object-cover transition-transform group-hover:scale-105"
                />
              </button>
            ))}
          </div>
        )}
      </CardContent>

      {/* ── Lightbox overlay ─────────────────────────────────────────── */}
      {lightboxPhoto && (
        <dialog
          open
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/80 m-0 p-0 max-w-none max-h-none w-screen h-screen border-none"
          onClick={() => setLightboxIndex(null)}
          onKeyDown={(e) => {
            if (e.key === "Escape") setLightboxIndex(null);
          }}
          aria-label={t("photos.lightboxLabel")}
        >
          <div
            className="relative max-h-[90vh] max-w-[90vw]"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={() => {}}
            role="presentation"
          >
            <img
              src={convertFileSrc(lightboxPhoto.file_path)}
              alt={lightboxPhoto.caption ?? lightboxPhoto.file_name}
              className="max-h-[85vh] max-w-[85vw] rounded-md object-contain"
            />

            {/* Navigation arrows */}
            {lightboxIndex !== null && lightboxIndex > 0 && (
              <button
                type="button"
                className="absolute left-2 top-1/2 -translate-y-1/2 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
                onClick={(e) => {
                  e.stopPropagation();
                  setLightboxIndex(lightboxIndex - 1);
                }}
              >
                <ChevronLeft className="h-5 w-5" />
              </button>
            )}
            {lightboxIndex !== null && lightboxIndex < photos.length - 1 && (
              <button
                type="button"
                className="absolute right-2 top-1/2 -translate-y-1/2 rounded-full bg-black/50 p-2 text-white hover:bg-black/70"
                onClick={(e) => {
                  e.stopPropagation();
                  setLightboxIndex(lightboxIndex + 1);
                }}
              >
                <ChevronRight className="h-5 w-5" />
              </button>
            )}

            {/* Close button */}
            <button
              type="button"
              className="absolute right-2 top-2 rounded-full bg-black/50 p-1.5 text-white hover:bg-black/70"
              onClick={() => setLightboxIndex(null)}
            >
              <X className="h-4 w-4" />
            </button>

            {/* Delete button (permission-gated) */}
            <PermissionGate permission="eq.manage">
              <button
                type="button"
                className="absolute bottom-3 right-3 rounded-md bg-status-danger/90 px-3 py-1.5 text-xs text-white hover:bg-status-danger"
                onClick={(e) => {
                  e.stopPropagation();
                  setDeleteConfirmId(lightboxPhoto.id);
                }}
              >
                <Trash2 className="mr-1 inline-block h-3 w-3" />
                {t("photos.delete")}
              </button>
            </PermissionGate>

            {/* Caption / filename */}
            <div className="absolute bottom-3 left-3 rounded-md bg-black/50 px-2 py-1 text-xs text-white">
              {lightboxPhoto.caption ?? lightboxPhoto.file_name}
            </div>
          </div>
        </dialog>
      )}

      {/* ── Delete confirm dialog ────────────────────────────────────── */}
      <Dialog
        open={deleteConfirmId !== null}
        onOpenChange={(isOpen) => !isOpen && setDeleteConfirmId(null)}
      >
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("photos.deleteConfirmTitle")}</DialogTitle>
            <DialogDescription>{t("photos.deleteConfirmDescription")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteConfirmId(null)} disabled={deleting}>
              {t("decommission.cancel")}
            </Button>
            <Button
              variant="destructive"
              disabled={deleting}
              onClick={() => deleteConfirmId !== null && void handleDelete(deleteConfirmId)}
            >
              {deleting && <Loader2 className="mr-2 h-3.5 w-3.5 animate-spin" />}
              {t("photos.delete")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </Card>
  );
}
