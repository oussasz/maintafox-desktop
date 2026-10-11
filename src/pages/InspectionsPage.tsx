/**
 * Inspection rounds (PRD §6.25): field runner for technicians, round review for supervisors,
 * checklist templates and reliability signals.
 */
import { ClipboardCheck } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { InspectionRoundsPanel } from "@/components/inspections/InspectionRoundsPanel";
import { InspectionRunner } from "@/components/inspections/InspectionRunner";
import { InspectionSignalsPanel } from "@/components/inspections/InspectionSignalsPanel";
import { InspectionTemplatesPanel } from "@/components/inspections/InspectionTemplatesPanel";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

export function InspectionsPage() {
  const { t } = useTranslation("inspections");
  const [openRoundId, setOpenRoundId] = useState<number | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  return (
    <ModulePageShell
      icon={ClipboardCheck}
      title={t("page.title")}
      description={t("page.description")}
      bodyClassName="space-y-6 overflow-auto p-4"
    >
      {openRoundId != null ? (
        <InspectionRunner
          roundId={openRoundId}
          onClose={() => {
            setOpenRoundId(null);
            setRefreshKey((k) => k + 1);
          }}
          onChanged={() => setRefreshKey((k) => k + 1)}
        />
      ) : (
        <Tabs defaultValue="rounds" className="space-y-4">
          <TabsList className="h-12">
            <TabsTrigger value="rounds" className="min-h-10 px-4 text-base">
              {t("tabs.rounds")}
            </TabsTrigger>
            <TabsTrigger value="templates" className="min-h-10 px-4 text-base">
              {t("tabs.templates")}
            </TabsTrigger>
            <TabsTrigger value="signals" className="min-h-10 px-4 text-base">
              {t("tabs.signals")}
            </TabsTrigger>
          </TabsList>
          <TabsContent value="rounds">
            <InspectionRoundsPanel onOpenRound={setOpenRoundId} refreshKey={refreshKey} />
          </TabsContent>
          <TabsContent value="templates">
            <InspectionTemplatesPanel />
          </TabsContent>
          <TabsContent value="signals">
            <InspectionSignalsPanel />
          </TabsContent>
        </Tabs>
      )}
    </ModulePageShell>
  );
}
