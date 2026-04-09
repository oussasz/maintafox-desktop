/**
 * ReferenceManagerPage.tsx
 *
 * Two-pane workspace for governing reference domains, sets, and values.
 * Left: DomainBrowserPanel (domain → set hierarchy).
 * Right: Value editor area (empty state until a set is selected;
 *        ValueEditorTable patched in by File 02, Sprint S4).
 *
 * Phase 2 – Sub-phase 03 – Sprint S4 (GAP REF-01).
 */

import { AlertTriangle, Database, Download, Plus, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { DomainBrowserPanel } from "@/components/lookups/DomainBrowserPanel";
import { ReferenceImportWizard } from "@/components/lookups/ReferenceImportWizard";
import { ReferenceValueEditor } from "@/components/lookups/ReferenceValueEditor";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { usePermissions } from "@/hooks/use-permissions";
import { createDraftReferenceSet } from "@/services/reference-service";
import { useReferenceManagerStore } from "@/stores/reference-manager-store";
import type { ReferenceDomain } from "@shared/ipc-types";

// ── Status label key mapping (shared with DomainBrowserPanel) ─────────────

const STATUS_LABEL_KEY = {
  draft: "browser.status.draft",
  validated: "browser.status.validated",
  published: "browser.status.published",
  superseded: "browser.status.superseded",
} as const;

type SetStatus = keyof typeof STATUS_LABEL_KEY;

function statusLabelKey(status: string) {
  return STATUS_LABEL_KEY[status as SetStatus] ?? "browser.status.draft";
}

export function ReferenceManagerPage() {
  const { t } = useTranslation("reference");
  const { can, isLoading: permLoading } = usePermissions();

  const domains = useReferenceManagerStore((s) => s.domains);
  const domainsLoading = useReferenceManagerStore((s) => s.domainsLoading);
  const error = useReferenceManagerStore((s) => s.error);
  const selectedDomainId = useReferenceManagerStore((s) => s.selectedDomainId);
  const selectedSetId = useReferenceManagerStore((s) => s.selectedSetId);
  const setsMap = useReferenceManagerStore((s) => s.setsMap);
  const loadDomains = useReferenceManagerStore((s) => s.loadDomains);
  const loadSetsForDomain = useReferenceManagerStore((s) => s.loadSetsForDomain);

  const [importOpen, setImportOpen] = useState(false);

  // Initial load
  useEffect(() => {
    void loadDomains();
  }, [loadDomains]);

  // ── Breadcrumb segments ─────────────────────────────────────────────────

  const selectedDomain = selectedDomainId ? domains.find((d) => d.id === selectedDomainId) : null;

  const selectedSet =
    selectedSetId && selectedDomainId
      ? setsMap[selectedDomainId]?.find((s) => s.id === selectedSetId)
      : null;

  // ── Callbacks ───────────────────────────────────────────────────────────

  const handleRefresh = useCallback(() => {
    void loadDomains();
    if (selectedDomainId) {
      void loadSetsForDomain(selectedDomainId);
    }
  }, [loadDomains, loadSetsForDomain, selectedDomainId]);

  const handleCreateDraftSet = useCallback(
    async (domainId: number) => {
      try {
        await createDraftReferenceSet(domainId);
        void loadSetsForDomain(domainId);
      } catch {
        // Error handled by store / toast in future iteration
      }
    },
    [loadSetsForDomain],
  );

  const handleRenameDomain = useCallback((_domain: ReferenceDomain) => {
    // Rename dialog will be wired in a follow-up sprint
  }, []);

  // ── Permission gate ─────────────────────────────────────────────────────

  if (permLoading) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  if (!can("ref.view")) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="text-center space-y-3">
          <AlertTriangle className="h-8 w-8 mx-auto text-status-danger" />
          <p className="text-sm text-status-danger">{t("page.noPermission")}</p>
        </div>
      </div>
    );
  }

  // ── Loading state (initial) ─────────────────────────────────────────────

  if (domainsLoading && domains.length === 0) {
    return (
      <div className="flex h-full items-center justify-center">
        <div className="h-6 w-6 animate-spin rounded-full border-2 border-surface-3 border-t-primary" />
      </div>
    );
  }

  // ── Error state (fatal) ─────────────────────────────────────────────────

  if (error && domains.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <div className="text-center space-y-3">
          <AlertTriangle className="h-8 w-8 mx-auto text-status-danger" />
          <p className="text-sm text-status-danger">{error}</p>
          <Button variant="outline" size="sm" onClick={() => void loadDomains()}>
            {t("page.retry")}
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      {/* ── Page header ────────────────────────────────────────────────── */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-surface-border">
        <div className="flex items-center gap-3 min-w-0">
          <Database className="h-5 w-5 shrink-0 text-text-muted" />

          {/* Breadcrumb */}
          <nav className="flex items-center gap-1.5 text-sm min-w-0">
            <span className="font-semibold text-text-primary">{t("page.title")}</span>
            {selectedDomain && (
              <>
                <Separator orientation="vertical" className="h-4 mx-1" />
                <span className="text-text-secondary truncate">{selectedDomain.name}</span>
              </>
            )}
            {selectedSet && (
              <>
                <Separator orientation="vertical" className="h-4 mx-1" />
                <span className="text-text-secondary">v{selectedSet.version_no}</span>
                <Badge
                  variant={
                    selectedSet.status === "published"
                      ? "default"
                      : selectedSet.status === "draft"
                        ? "secondary"
                        : "outline"
                  }
                  className="text-[10px] ml-1"
                >
                  {t(statusLabelKey(selectedSet.status))}
                </Badge>
              </>
            )}
          </nav>

          <Badge variant="secondary" className="text-xs shrink-0">
            {t("page.domainCount", { count: domains.length })}
          </Badge>
        </div>

        {/* Action buttons */}
        <div className="flex items-center gap-2">
          <PermissionGate permission="ref.manage">
            <Button
              variant="outline"
              size="sm"
              className="gap-1.5"
              onClick={() => setImportOpen(true)}
              disabled={!selectedDomainId || !selectedSetId}
            >
              <Download className="h-3.5 w-3.5" />
              {t("page.import")}
            </Button>
            <Button variant="outline" size="sm" className="gap-1.5">
              <Plus className="h-3.5 w-3.5" />
              {t("page.newDomain")}
            </Button>
          </PermissionGate>
          <Button
            variant="outline"
            size="sm"
            onClick={handleRefresh}
            disabled={domainsLoading}
            className="gap-1.5"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${domainsLoading ? "animate-spin" : ""}`} />
            {t("page.refresh")}
          </Button>
        </div>
      </div>

      {/* ── Two-pane workspace ─────────────────────────────────────────── */}
      <div className="flex flex-1 min-h-0">
        {/* Left pane: domain browser */}
        <DomainBrowserPanel
          onCreateDraftSet={(domainId) => void handleCreateDraftSet(domainId)}
          onRenameDomain={handleRenameDomain}
        />

        {/* Right pane: value editor area */}
        <main className="flex-1 min-w-0">
          {selectedSetId && selectedDomainId ? (
            <ReferenceValueEditor setId={selectedSetId} domainId={selectedDomainId} />
          ) : (
            <div className="flex h-full items-center justify-center p-6">
              <div className="text-center space-y-2">
                <Database className="h-10 w-10 mx-auto text-text-muted/40" />
                <p className="text-sm text-text-muted">{t("page.emptyState")}</p>
              </div>
            </div>
          )}
        </main>
      </div>

      {/* Import wizard */}
      {selectedDomainId && selectedSetId && (
        <ReferenceImportWizard
          domainId={selectedDomainId}
          targetSetId={selectedSetId}
          open={importOpen}
          onOpenChange={setImportOpen}
          onComplete={handleRefresh}
        />
      )}
    </div>
  );
}
