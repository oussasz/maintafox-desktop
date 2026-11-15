/**
 * The machine's documents (manual, procedures, safety protocols, certificates) shown on the work
 * order, read from the document library by the order's equipment (L7-16). The order's own
 * attachments (photos, reports) stay in WoAttachmentPanel; documents are added from the asset.
 */

import { BookOpen } from "lucide-react";
import { useTranslation } from "react-i18next";

import { AssetLibraryDocumentList } from "@/components/assets/detail/AssetLibraryDocumentList";
import { DetailSectionCard } from "@/components/detail";

export function WoAssetDocumentsCard({
  equipmentId,
  assetLabel,
}: {
  equipmentId: number | null;
  assetLabel: string;
}) {
  const { t } = useTranslation("equipment");
  if (equipmentId == null) return null;
  return (
    <DetailSectionCard title={t("documents.onWoTitle", { asset: assetLabel })} icon={BookOpen}>
      <p className="mb-2 text-xs text-text-muted">{t("documents.onWoHint")}</p>
      <AssetLibraryDocumentList assetId={equipmentId} />
    </DetailSectionCard>
  );
}
