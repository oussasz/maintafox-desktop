import { ScrollText } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ActivityFeedPanel } from "@/components/activity/ActivityFeedPanel";
import { AuditLogViewer } from "@/components/activity/AuditLogViewer";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export function ActivityPage() {
  const { t } = useTranslation("activity");

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-surface-border px-6 py-3">
        <div className="flex items-center gap-3">
          <ScrollText className="h-5 w-5 text-text-muted" aria-hidden />
          <div>
            <h1 className="text-xl font-semibold text-text-primary">{t("page.title")}</h1>
            <p className="mt-0.5 text-sm text-text-muted">{t("page.subtitle")}</p>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-6">
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
  );
}
