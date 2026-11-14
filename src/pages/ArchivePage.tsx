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
      {/* L7-19: say what is (and is not) archived, where closed records are, and how to restore. */}
      <section
        aria-labelledby="archive-about-title"
        className="mb-4 space-y-1 rounded-md border border-tone-info-border bg-tone-info-bg px-3 py-2 text-sm text-tone-info-fg"
      >
        <h2 id="archive-about-title" className="font-medium">
          {t("page.about.title")}
        </h2>
        <p>{t("page.about.closed")}</p>
        <p>{t("page.about.what")}</p>
        <p>{t("page.about.restore", { button: t("detail.restore") })}</p>
      </section>
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
