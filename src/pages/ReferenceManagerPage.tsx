/**
 * ReferenceManagerPage.tsx
 *
 * Reference data (catalogues) of the plant: equipment classes and families, failure codes,
 * criticality, skills, work-order types, … Two-pane workspace inside the standard module shell:
 * left the domain → version tree, right the values of the selected version (ReferenceValueEditor)
 * or the live table of another module (synthetic adapters).
 */

import { Database } from "lucide-react";
import { useCallback, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { RefreshButton } from "@/components/layout/ModuleHeaderControls";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { DomainBrowserPanel } from "@/components/lookups/DomainBrowserPanel";
import { ReferenceValueEditor } from "@/components/lookups/ReferenceValueEditor";
import { InventoryArticleFamilyHost } from "@/components/lookups/adapters/InventoryArticleFamilyHost";
import { InventoryTaxCategoryHost } from "@/components/lookups/adapters/InventoryTaxCategoryHost";
import { NotificationRulesHost } from "@/components/lookups/adapters/NotificationRulesHost";
import { PermitTypesHost } from "@/components/lookups/adapters/PermitTypesHost";
import { UrgencyScaleHost } from "@/components/lookups/adapters/UrgencyScaleHost";
import { WorkOrderStatusesHost } from "@/components/lookups/adapters/WorkOrderStatusesHost";
import {
  REFERENCE_SET_STATUS_TONE,
  isSyntheticReferenceId,
  isWorkingCatalogCategory,
  pendingCandidateSet,
  referenceDomainLabel,
  referenceSetStatusLabelKey,
} from "@/components/lookups/reference-set-ui";
import { Badge } from "@/components/ui/badge";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusChip } from "@/components/ui/status-chip";
import { mfLayout } from "@/design-system/tokens";
import { toneOf } from "@/design-system/tone";
import { usePermissions } from "@/hooks/use-permissions";
import { LIST_PARAM, textParam, withoutParams } from "@/lib/list-links";
import { createDraftReferenceSet } from "@/services/reference-service";
import {
  INVENTORY_ARTICLE_FAMILY_DOMAIN_ID,
  INVENTORY_TAX_CATEGORY_DOMAIN_ID,
  NOTIFICATION_RULES_DOMAIN_ID,
  PERMIT_TYPES_DOMAIN_ID,
  URGENCY_SCALE_DOMAIN_ID,
  WORK_ORDER_STATUSES_DOMAIN_ID,
  useReferenceManagerStore,
} from "@/stores/reference-manager-store";
import { refreshUrgencyScale } from "@/stores/urgency-scale-store";
import { refreshWorkOrderTypesCatalog } from "@/stores/work-order-types-catalog-store";
import { P } from "@shared/rbac/permissions.generated";

/** Right pane for the live tables of other modules shown in the tree (no versions). */
function SyntheticDomainHost({ domainId }: { domainId: number }) {
  switch (domainId) {
    case INVENTORY_ARTICLE_FAMILY_DOMAIN_ID:
      return <InventoryArticleFamilyHost />;
    case INVENTORY_TAX_CATEGORY_DOMAIN_ID:
      return <InventoryTaxCategoryHost />;
    case URGENCY_SCALE_DOMAIN_ID:
      return <UrgencyScaleHost />;
    case WORK_ORDER_STATUSES_DOMAIN_ID:
      return <WorkOrderStatusesHost />;
    case NOTIFICATION_RULES_DOMAIN_ID:
      return <NotificationRulesHost />;
    case PERMIT_TYPES_DOMAIN_ID:
      return <PermitTypesHost />;
    default:
      return null;
  }
}

export function ReferenceManagerPage() {
  const { t } = useTranslation("reference");
  const { can, isLoading: permLoading } = usePermissions();
  const label = useCallback((key: string) => String(t(key as "page.title")), [t]);

  const domains = useReferenceManagerStore((s) => s.domains);
  const domainsLoading = useReferenceManagerStore((s) => s.domainsLoading);
  const error = useReferenceManagerStore((s) => s.error);
  const selectedDomainId = useReferenceManagerStore((s) => s.selectedDomainId);
  const selectedSetId = useReferenceManagerStore((s) => s.selectedSetId);
  const setsMap = useReferenceManagerStore((s) => s.setsMap);
  const loadDomains = useReferenceManagerStore((s) => s.loadDomains);
  const loadSetsForDomain = useReferenceManagerStore((s) => s.loadSetsForDomain);
  const selectSet = useReferenceManagerStore((s) => s.selectSet);
  const selectDomain = useReferenceManagerStore((s) => s.selectDomain);

  useEffect(() => {
    void loadDomains();
  }, [loadDomains]);

  // A link may open one list (`?catalog=<code>`, e.g. from the module that uses it): once the
  // tree is loaded the list is selected and the parameter leaves the address.
  const [searchParams, setSearchParams] = useSearchParams();
  const linkedCatalog = textParam(searchParams.get(LIST_PARAM.catalog));
  useEffect(() => {
    if (!linkedCatalog || domainsLoading || domains.length === 0) return;
    const domain = domains.find((d) => d.code.toUpperCase() === linkedCatalog.toUpperCase());
    if (domain) {
      if (isSyntheticReferenceId(domain.id)) selectSet(domain.id, domain.id);
      else selectDomain(domain.id);
    }
    setSearchParams(withoutParams(searchParams, LIST_PARAM.catalog), { replace: true });
  }, [
    linkedCatalog,
    domains,
    domainsLoading,
    searchParams,
    setSearchParams,
    selectSet,
    selectDomain,
  ]);

  const selectedDomain =
    selectedDomainId != null ? (domains.find((d) => d.id === selectedDomainId) ?? null) : null;
  const selectedSet =
    selectedSetId != null && selectedDomainId != null
      ? (setsMap[selectedDomainId]?.find((s) => s.id === selectedSetId) ?? null)
      : null;
  const synthetic = isSyntheticReferenceId(selectedDomainId);

  const handleRefresh = useCallback(() => {
    void loadDomains();
    void refreshWorkOrderTypesCatalog();
    void refreshUrgencyScale();
    if (selectedDomainId != null) {
      void loadSetsForDomain(selectedDomainId);
    }
  }, [loadDomains, loadSetsForDomain, selectedDomainId]);

  /**
   * "New draft" from the tree. On failure the global toast explains why (for instance a draft
   * already exists); the tree is then reloaded and an existing draft is opened.
   */
  const handleCreateDraftSet = useCallback(
    async (domainId: number) => {
      try {
        const draft = await createDraftReferenceSet(domainId);
        await loadSetsForDomain(domainId);
        selectSet(draft.id, domainId);
      } catch {
        await loadSetsForDomain(domainId);
        const candidate = pendingCandidateSet(
          useReferenceManagerStore.getState().setsMap[domainId] ?? [],
        );
        if (candidate) selectSet(candidate.id, domainId);
      }
    },
    [loadSetsForDomain, selectSet],
  );

  if (permLoading) {
    return <LoadingState className="h-full" />;
  }

  if (!can(P.REF_VIEW)) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <EmptyState title={t("page.noPermission")} description={t("page.noPermissionHint")} />
      </div>
    );
  }

  const headerLeft = (
    <div className="flex min-w-0 flex-wrap items-center gap-2">
      <Database className="h-5 w-5 shrink-0 text-text-muted" aria-hidden />
      <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
      {domains.length > 0 ? (
        <Badge
          variant="secondary"
          className={mfLayout.moduleCount}
          title={t("page.domainCount", { count: domains.length })}
        >
          {domains.length}
        </Badge>
      ) : null}
      {selectedDomain ? (
        <>
          <span className="text-text-muted" aria-hidden>
            /
          </span>
          <span className="truncate text-sm text-text-secondary">
            {referenceDomainLabel(selectedDomain, label)}
          </span>
        </>
      ) : null}
      {selectedSet &&
      !synthetic &&
      !isWorkingCatalogCategory(selectedDomain?.governance_category) ? (
        <>
          <span className="text-sm text-text-secondary">
            {t("editor.version", { version: selectedSet.version_no })}
          </span>
          <StatusChip
            label={label(referenceSetStatusLabelKey(selectedSet.status))}
            tone={toneOf(REFERENCE_SET_STATUS_TONE, selectedSet.status)}
          />
        </>
      ) : null}
    </div>
  );

  let workspace: React.ReactNode;
  if (domainsLoading && domains.length === 0) {
    workspace = <LoadingState className="flex-1" />;
  } else if (error && domains.length === 0) {
    workspace = (
      <div className="flex-1 p-6">
        <ErrorState error={error} onRetry={() => void loadDomains()} retrying={domainsLoading} />
      </div>
    );
  } else {
    workspace = (
      <div className="flex min-h-0 flex-1">
        <DomainBrowserPanel onCreateDraftSet={(domainId) => void handleCreateDraftSet(domainId)} />
        <main className="min-w-0 flex-1" aria-label={t("page.valuesPane")}>
          {synthetic && selectedDomainId != null && selectedSetId != null ? (
            <SyntheticDomainHost domainId={selectedDomainId} />
          ) : selectedSetId != null && selectedDomainId != null ? (
            <ReferenceValueEditor setId={selectedSetId} domainId={selectedDomainId} />
          ) : (
            <div className="flex h-full items-center justify-center p-6">
              <EmptyState
                icon={<Database className="h-10 w-10" aria-hidden />}
                title={t("page.emptyState")}
                description={t("page.emptyStateHint")}
              />
            </div>
          )}
        </main>
      </div>
    );
  }

  return (
    <ModulePageShell
      headerLeft={headerLeft}
      actions={<RefreshButton onRefresh={handleRefresh} loading={domainsLoading} />}
      bodyClassName={null}
    >
      {workspace}
    </ModulePageShell>
  );
}
