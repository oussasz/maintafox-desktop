/**
 * Qualifications & training (PRD §6.20) — benchmark SAP HCM qualifications / SAP PM task-list
 * requirements and Maximo labour qualifications:
 * - matrix of people × certifications and skills with expiry colours;
 * - expiries and renewal plan (grouped by certification, plan a session for those concerned);
 * - training sessions, attendance and results (a pass issues the certification);
 * - what jobs require (work-order types, permit types, LOTO roles);
 * - the certification catalogue; the log of supervisor overrides; the user's own record.
 */
import { GraduationCap } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { CertificationCatalogPanel } from "@/components/personnel/qualification/CertificationCatalogPanel";
import { ExpiryRenewalPanel } from "@/components/personnel/qualification/ExpiryRenewalPanel";
import { MyQualificationsPanel } from "@/components/personnel/qualification/MyQualificationsPanel";
import { OverrideLogPanel } from "@/components/personnel/qualification/OverrideLogPanel";
import { QualificationMatrixView } from "@/components/personnel/qualification/QualificationMatrixView";
import { QualificationRequirementsPanel } from "@/components/personnel/qualification/QualificationRequirementsPanel";
import { TrainingSessionsPanel } from "@/components/personnel/qualification/TrainingSessionsPanel";
import { useQualificationLookups } from "@/components/personnel/qualification/use-qualification-lookups";
import { ErrorState } from "@/components/ui/data-state";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { usePermissions } from "@/hooks/use-permissions";
import { P } from "@shared/rbac/permissions.generated";

type TrainingTab =
  | "matrix"
  | "expiry"
  | "sessions"
  | "requirements"
  | "catalog"
  | "overrides"
  | "mine";

export function TrainingPage() {
  const { t } = useTranslation("training");
  const { can } = usePermissions();
  const canView = can(P.TRN_VIEW);
  const lookups = useQualificationLookups(canView);
  const [tab, setTab] = useState<TrainingTab>(canView ? "matrix" : "mine");
  const [refreshKey, setRefreshKey] = useState(0);
  const [presetCertType, setPresetCertType] = useState<number | null>(null);

  const refreshAll = useCallback(() => {
    setRefreshKey((k) => k + 1);
    void lookups.reload();
  }, [lookups]);

  const planSession = useCallback((certificationTypeId: number) => {
    setPresetCertType(certificationTypeId);
    setTab("sessions");
  }, []);

  const tabs: { value: TrainingTab; label: string; visible: boolean }[] = [
    { value: "matrix", label: t("tabs.matrix"), visible: canView },
    { value: "expiry", label: t("tabs.expiry"), visible: canView },
    { value: "sessions", label: t("tabs.sessions"), visible: canView },
    { value: "requirements", label: t("tabs.requirements"), visible: canView },
    { value: "catalog", label: t("tabs.catalog"), visible: canView },
    { value: "overrides", label: t("tabs.overrides"), visible: canView },
    { value: "mine", label: t("tabs.mine"), visible: true },
  ];

  return (
    <ModulePageShell
      icon={GraduationCap}
      title={t("page.title")}
      description={t("page.description")}
      actions={<RefreshButton onRefresh={refreshAll} loading={lookups.loading} />}
      bodyClassName="space-y-4 overflow-auto p-4"
    >
      {lookups.error && canView ? (
        <ErrorState
          error={lookups.error}
          onRetry={() => void lookups.reload()}
          retrying={lookups.loading}
        />
      ) : null}
      <Tabs value={tab} onValueChange={(v) => setTab(v as TrainingTab)} className="space-y-4">
        <TabsList className="h-12 flex-wrap">
          {tabs
            .filter((x) => x.visible)
            .map((x) => (
              <TabsTrigger key={x.value} value={x.value} className="min-h-10 px-4 text-base">
                {x.label}
              </TabsTrigger>
            ))}
        </TabsList>
        {canView ? (
          <>
            <TabsContent value="matrix">
              <QualificationMatrixView
                teams={lookups.teams}
                companies={lookups.companies}
                refreshKey={refreshKey}
              />
            </TabsContent>
            <TabsContent value="expiry">
              <ExpiryRenewalPanel refreshKey={refreshKey} onPlanSession={planSession} />
            </TabsContent>
            <TabsContent value="sessions">
              <TrainingSessionsPanel
                people={lookups.people}
                certTypes={lookups.certTypes}
                presetCertificationTypeId={presetCertType}
                onPresetConsumed={() => setPresetCertType(null)}
                onChanged={() => setRefreshKey((k) => k + 1)}
              />
            </TabsContent>
            <TabsContent value="requirements">
              <QualificationRequirementsPanel
                certTypes={lookups.certTypes}
                skills={lookups.skills}
              />
            </TabsContent>
            <TabsContent value="catalog">
              <CertificationCatalogPanel
                certTypes={lookups.certTypes}
                loading={lookups.loading}
                onChanged={refreshAll}
              />
            </TabsContent>
            <TabsContent value="overrides">
              <OverrideLogPanel refreshKey={refreshKey} />
            </TabsContent>
          </>
        ) : null}
        <TabsContent value="mine">
          <MyQualificationsPanel />
        </TabsContent>
      </Tabs>
    </ModulePageShell>
  );
}
