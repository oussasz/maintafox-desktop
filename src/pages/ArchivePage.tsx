import { Archive } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ArchiveExplorer } from "@/components/archive/ArchiveExplorer";
import { RetentionPolicyPanel } from "@/components/archive/RetentionPolicyPanel";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { mfLayout } from "@/design-system/tokens";

export function ArchivePage() {
  const { t } = useTranslation("archive");

  return (
    <div className={mfLayout.moduleRoot}>
      <div className={mfLayout.moduleHeader}>
        <div className={mfLayout.moduleTitleRow}>
          <Archive className={mfLayout.moduleHeaderIcon} aria-hidden />
          <div className="min-w-0">
            <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
            <p className="mt-0.5 text-sm text-text-muted">{t("page.subtitle")}</p>
          </div>
        </div>
      </div>

      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>
          <div className={mfLayout.moduleWorkspaceBody}>
            <Tabs defaultValue="explorer" className="w-full">
              <TabsList className="grid w-full max-w-md grid-cols-2">
                <TabsTrigger value="explorer">{t("tabs.explorer")}</TabsTrigger>
                <TabsTrigger value="retention">{t("tabs.retention")}</TabsTrigger>
              </TabsList>
              <TabsContent value="explorer" className="mt-4">
                <ArchiveExplorer />
              </TabsContent>
              <TabsContent value="retention" className="mt-4">
                <RetentionPolicyPanel />
              </TabsContent>
            </Tabs>
          </div>
        </div>
      </div>
    </div>
  );
}
