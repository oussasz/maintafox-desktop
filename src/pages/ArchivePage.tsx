import { Archive } from "lucide-react";
import { useTranslation } from "react-i18next";

import { ArchiveExplorer } from "@/components/archive/ArchiveExplorer";
import { RetentionPolicyPanel } from "@/components/archive/RetentionPolicyPanel";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export function ArchivePage() {
  const { t } = useTranslation("archive");

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-surface-border px-6 py-3">
        <div className="flex items-center gap-3">
          <Archive className="h-5 w-5 text-text-muted" aria-hidden />
          <div>
            <h1 className="text-xl font-semibold text-text-primary">{t("page.title")}</h1>
            <p className="mt-0.5 text-sm text-text-muted">{t("page.subtitle")}</p>
          </div>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-6">
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
  );
}
