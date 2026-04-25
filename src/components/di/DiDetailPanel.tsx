/**
 * DiDetailPanel.tsx
 *
 * Tabbed detail panel for a single DI: attachments, formal state log, and activity.
 */

import { useTranslation } from "react-i18next";

import { DiAttachmentPanel } from "@/components/di/DiAttachmentPanel";
import { DiAuditTimeline } from "@/components/di/DiAuditTimeline";
import { DiStateTransitionList } from "@/components/di/DiStateTransitionList";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui";
import type { DiTransitionRow, InterventionRequest } from "@shared/ipc-types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface DiDetailPanelProps {
  di: InterventionRequest;
  /** Formal state machine log (`di_state_transition_log`); from `get_di`. */
  transitions: DiTransitionRow[];
  canUploadAttachment: boolean;
  canDeleteAttachment: boolean;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function DiDetailPanel({
  di,
  transitions,
  canUploadAttachment,
  canDeleteAttachment,
}: DiDetailPanelProps) {
  const { t } = useTranslation("di");

  return (
    <Tabs defaultValue="attachments" className="w-full">
      <TabsList>
        <TabsTrigger value="attachments">{t("detail.tabs.attachments")}</TabsTrigger>
        <TabsTrigger value="stateLog">{t("detail.tabs.stateLog")}</TabsTrigger>
        <TabsTrigger value="activity">{t("detail.tabs.activity")}</TabsTrigger>
      </TabsList>

      <TabsContent value="attachments" className="mt-4">
        <DiAttachmentPanel
          diId={di.id}
          canUpload={canUploadAttachment}
          canDelete={canDeleteAttachment}
        />
      </TabsContent>

      <TabsContent value="stateLog" className="mt-4">
        <DiStateTransitionList transitions={transitions} />
      </TabsContent>

      <TabsContent value="activity" className="mt-4">
        <DiAuditTimeline diId={di.id} />
      </TabsContent>
    </Tabs>
  );
}
