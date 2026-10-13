import { useEffect } from "react";
import { useTranslation } from "react-i18next";

import { ActivityFeedPanel } from "@/components/activity/ActivityFeedPanel";
import { AuditLogViewer } from "@/components/activity/AuditLogViewer";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { i18n } from "@/i18n";

/**
 * Activity feed + audit log — embeddable in Settings (no page shell).
 */
export function ActivityLogSettingsSection() {
  const { t } = useTranslation("activity");

  useEffect(() => {
    void i18n.loadNamespaces(["activity"]);
  }, []);

  return (
    <div className="space-y-4">
      <div>
        <h3 className="text-base font-semibold text-text-primary">{t("page.title")}</h3>
        <p className="mt-1 text-sm text-text-muted">{t("page.subtitle")}</p>
      </div>
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
  );
}
