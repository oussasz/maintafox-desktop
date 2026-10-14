import { Archive } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ArchiveExplorer } from "@/components/archive/ArchiveExplorer";
import { RetentionPolicyPanel } from "@/components/archive/RetentionPolicyPanel";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export function ArchivePage() {
  const { t } = useTranslation("archive");

  return (
    <ModulePageShell icon={Archive} title={t("page.title")} description={t("page.subtitle")}>
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
    </ModulePageShell>
  );
}
