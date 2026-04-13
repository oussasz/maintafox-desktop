/**
 * DiDetailPanel.tsx
 *
 * Tabbed detail panel for a single DI, hosting sub-panels such as
 * attachments and audit timeline.
 * Phase 2 – Sub-phase 04 – File 04 – Sprint S3.
 */

import { DiAttachmentPanel } from "@/components/di/DiAttachmentPanel";
import { DiAuditTimeline } from "@/components/di/DiAuditTimeline";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiDetailPanelProps {
  di: InterventionRequest;
  canUploadAttachment: boolean;
  canDeleteAttachment: boolean;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiDetailPanel({
  di,
  canUploadAttachment,
  canDeleteAttachment,
}: DiDetailPanelProps) {
  return (
    <Tabs defaultValue="attachments" className="w-full">
      <TabsList>
        <TabsTrigger value="attachments">Pièces jointes</TabsTrigger>
        <TabsTrigger value="audit">Audit Trail</TabsTrigger>
      </TabsList>

      <TabsContent value="attachments" className="mt-4">
        <DiAttachmentPanel
          diId={di.id}
          canUpload={canUploadAttachment}
          canDelete={canDeleteAttachment}
        />
      </TabsContent>

      <TabsContent value="audit" className="mt-4">
        <DiAuditTimeline diId={di.id} />
      </TabsContent>
    </Tabs>
  );
}
