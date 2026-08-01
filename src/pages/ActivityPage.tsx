import { ScrollText } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ActivityFeedPanel } from "@/components/activity/ActivityFeedPanel";
import { AuditLogViewer } from "@/components/activity/AuditLogViewer";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";

export function ActivityPage() {
  const { t } = useTranslation("activity");

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <ScrollText className={mfLayout.moduleHeaderIcon} aria-hidden />
          <div className="min-w-0">
            <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
            <p className="mt-0.5 text-sm text-text-muted">{t("page.subtitle")}</p>
          </div>
        </div>
      </div>

      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          <div className={mfLayout.moduleWorkspaceBody}>
            <Tabs defaultValue="activity" className="w-full">
              <TabsList className="grid w-full max-w-md grid-cols-2">
                <TabsTrigger value="activity">{t("tabs.activity")}</TabsTrigger>
                <TabsTrigger value="audit">{t("tabs.audit")}</TabsTrigger>
              </TabsList>
              <TabsContent value="activity" className="mt-4">
                <ActivityFeedPanel />
              </TabsContent>
              <TabsContent value="audit" className="mt-4">
                <AuditLogViewer />
              </TabsContent>
            </Tabs>
          </div>
        </div>
      </div>
    </div>
  );
}
