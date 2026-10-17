/**
 * PersonnelPortrait — loads personnel photo via IPC preview (data URL).
 * Mirrors AssetPhotoGallery's read_*_preview approach; does not use convertFileSrc.
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { EntityPortrait } from "@/components/detail/EntityPortrait";
import { describeAppError } from "@/lib/app-error";
import { readPersonnelPhotoPreview } from "@/services/personnel-service";

export interface PersonnelPortraitProps {
  personnelId: number;
  /** When null/empty, show empty silhouette without calling IPC. */
  photoPath: string | null | undefined;
  fullName: string;
  sizePx?: number;
}

function photoDataUrl(preview: { mime_type: string; data_base64: string }): string {
  return `data:${preview.mime_type};base64,${preview.data_base64}`;
}

export function PersonnelPortrait({
  personnelId,
  photoPath,
  fullName,
  sizePx,
}: PersonnelPortraitProps) {
  const { t } = useTranslation("personnel");
  const [src, setSrc] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const hasPath = Boolean(photoPath?.trim());

  useEffect(() => {
    if (!hasPath) {
      setSrc(null);
      setLoading(false);
      setError(null);
      return;
    }

    let cancelled = false;
    setSrc(null);
    setError(null);
    setLoading(true);

    void readPersonnelPhotoPreview(personnelId)
      .then((preview) => {
        if (!cancelled) setSrc(photoDataUrl(preview));
      })
      .catch((err) => {
        if (import.meta.env.DEV) {
          console.error("[PersonnelPortrait] read_personnel_photo_preview failed", {
            personnelId,
            photoPath,
            error: describeAppError(err).title,
          });
        }
        if (!cancelled) {
          setSrc(null);
          setError(t("detail.photo.loadFailed"));
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [hasPath, personnelId, photoPath, t]);

  return (
    <EntityPortrait
      src={src}
      alt={t("detail.photo.alt", { name: fullName })}
      loading={loading}
      error={error}
      empty={!hasPath}
      {...(sizePx !== undefined ? { sizePx } : {})}
    />
  );
}
