import { History, Loader2, Settings } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { LockoutPolicyPanel, PasswordPolicyPanel } from "@/components/admin/PasswordPolicyPanel";
import { DiagnosticsPanel } from "@/components/diagnostics/DiagnosticsPanel";
import { ModulePageShell } from "@/components/layout/ModulePageShell";
import { LicenseEnforcementPanel } from "@/components/license/LicenseEnforcementPanel";
import { ActivityLogSettingsSection } from "@/components/settings/ActivityLogSettingsSection";
import { BackupOperationsPanel } from "@/components/settings/BackupOperationsPanel";
import { ConnectionProfilesPanel } from "@/components/settings/ConnectionProfilesPanel";
import { DatabaseRecoveryKeyPanel } from "@/components/settings/DatabaseRecoveryKeyPanel";
import { DocumentHeaderSettingsPanel } from "@/components/settings/DocumentHeaderSettingsPanel";
import { NotificationsSettingsPanel } from "@/components/settings/NotificationsSettingsPanel";
import { PolicyEditorPanel } from "@/components/settings/PolicyEditorPanel";
import { ProfileSettingsPanel } from "@/components/settings/ProfileSettingsPanel";
import {
  type CurrencyListState,
  SettingsCatalogSections,
  formatSettingValue,
} from "@/components/settings/SettingsCatalogSections";
import {
  SETTINGS_TABS,
  SettingsCategorySidebar,
  type SettingsTabId,
} from "@/components/settings/SettingsCategorySidebar";
import { SiteRulesPanel } from "@/components/settings/SiteRulesPanel";
import { SyncFeedbackPanel } from "@/components/sync/SyncFeedbackPanel";
import { Timeline } from "@/components/timeline";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { i18n } from "@/i18n";
import { describeAppError } from "@/lib/app-error";
import {
  ensureSettingsDefaults,
  getSettingsOverview,
  listReferenceCurrencies,
  listSettingChangeEvents,
} from "@/services/settings-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { CurrencyOption, SettingsChangeEvent, SettingsOverview } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Sections of the General tab, in display order (settings come from the server catalog). */
const GENERAL_CATEGORIES = ["localization", "work_orders", "inventory", "reliability"] as const;
const BACKUP_CATEGORIES = ["backup"] as const;
const SETTINGS_TAB_IDS = new Set<SettingsTabId>(SETTINGS_TABS.map((tab) => tab.id));

function parseSettingsTab(raw: string | null): SettingsTabId {
  if (raw && SETTINGS_TAB_IDS.has(raw as SettingsTabId)) {
    return raw as SettingsTabId;
  }
  return "general";
}

function isAdmSettingsTab(tab: SettingsTabId): boolean {
  return SETTINGS_TABS.find((entry) => entry.id === tab)?.requiresAdmSettings === true;
}

export function SettingsPage() {
  const { t } = useTranslation("settings");
  const tAny = t as unknown as (key: string, options?: Record<string, unknown>) => string;
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canManageSettings = can(P.ADM_SETTINGS);
  const [searchParams, setSearchParams] = useSearchParams();

  const [activeTab, setActiveTab] = useState<SettingsTabId>(() =>
    parseSettingsTab(searchParams.get("tab")),
  );
  const [overview, setOverview] = useState<SettingsOverview | null>(null);
  const [currencyOptions, setCurrencyOptions] = useState<CurrencyOption[] | null>(null);
  const [currencyError, setCurrencyError] = useState<unknown>(null);
  const [auditLog, setAuditLog] = useState<SettingsChangeEvent[]>([]);
  const [auditLogError, setAuditLogError] = useState<unknown>(null);
  const [initialLoading, setInitialLoading] = useState(true);
  const [contentRefreshing, setContentRefreshing] = useState(false);
  const [restoringDefaults, setRestoringDefaults] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const didAutoEnsureRef = useRef(false);

  const visibleTabs = useMemo(
    () =>
      SETTINGS_TABS.filter((tab) => {
        if (!tab.requiresAdmSettings) return true;
        return canManageSettings;
      }),
    [canManageSettings],
  );

  const defaultTabForUser: SettingsTabId = canManageSettings ? "general" : "profile";

  useEffect(() => {
    if (permissionsLoading) return;
    const requested = parseSettingsTab(searchParams.get("tab"));
    const allowed = visibleTabs.some((tab) => tab.id === requested)
      ? requested
      : (visibleTabs[0]?.id ?? defaultTabForUser);
    setActiveTab(allowed);
    const urlTab = searchParams.get("tab");
    const needsUrlFix =
      allowed !== requested ||
      (allowed !== "general" && urlTab !== allowed) ||
      (allowed === "general" && canManageSettings && urlTab != null);
    if (needsUrlFix) {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (allowed === "general" && canManageSettings) {
            next.delete("tab");
          } else {
            next.set("tab", allowed);
          }
          return next;
        },
        { replace: true },
      );
    }
  }, [
    searchParams,
    visibleTabs,
    permissionsLoading,
    defaultTabForUser,
    canManageSettings,
    setSearchParams,
  ]);

  const handleSelectTab = useCallback(
    (tab: SettingsTabId) => {
      setActiveTab(tab);
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (tab === "general" && canManageSettings) {
            next.delete("tab");
          } else {
            next.set("tab", tab);
          }
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams, canManageSettings],
  );

  const definitions = overview?.definitions ?? [];
  const hasStoredSettings = definitions.some((d) => d.current_value_json != null);

  /** Données de référence › Devises; the currency picker stays disabled until it is loaded. */
  const loadCurrencies = useCallback(async () => {
    setCurrencyError(null);
    try {
      setCurrencyOptions(await listReferenceCurrencies());
    } catch (err) {
      setCurrencyOptions(null);
      setCurrencyError(err);
    }
  }, []);

  const loadAuditLog = useCallback(async () => {
    setAuditLogError(null);
    try {
      setAuditLog(await listSettingChangeEvents(20));
    } catch (err) {
      setAuditLogError(err);
    }
  }, []);

  const currencies: CurrencyListState = useMemo(
    () => ({
      options: currencyOptions,
      error: currencyError,
      onRetry: () => void loadCurrencies(),
    }),
    [currencyOptions, currencyError, loadCurrencies],
  );

  const loadAll = useCallback(
    async (opts?: { silent?: boolean; allowEnsure?: boolean }) => {
      const silent = opts?.silent === true;
      const allowEnsure = opts?.allowEnsure !== false;
      if (!silent) setContentRefreshing(true);
      try {
        let next = await getSettingsOverview();

        if (
          !next.definitions.some((d) => d.current_value_json != null) &&
          allowEnsure &&
          !didAutoEnsureRef.current
        ) {
          didAutoEnsureRef.current = true;
          await ensureSettingsDefaults();
          next = await getSettingsOverview();
        }

        setOverview(next);
        setError(null);
        // Each reports its own failure with a retry (never an empty list in its place).
        await Promise.all([loadCurrencies(), loadAuditLog()]);
      } catch (err) {
        setError(err);
      } finally {
        setInitialLoading(false);
        setContentRefreshing(false);
      }
    },
    [loadCurrencies, loadAuditLog],
  );

  useEffect(() => {
    void i18n.loadNamespaces(["settings", "admin", "activity"]);
  }, []);

  useEffect(() => {
    if (permissionsLoading) return;
    if (!canManageSettings) {
      setInitialLoading(false);
      setContentRefreshing(false);
      setError(null);
      setOverview(null);
      setAuditLog([]);
      setAuditLogError(null);
      return;
    }
    void loadAll({ allowEnsure: true });
  }, [loadAll, canManageSettings, permissionsLoading]);

  const onToast = useCallback(
    (msg: {
      title: string;
      description?: string;
      variant?: "default" | "destructive" | "success";
    }) => {
      // The app-wide toast host shows Settings confirmations like every other screen's.
      pushAppToast({
        title: msg.title,
        ...(msg.description !== undefined ? { description: msg.description } : {}),
        variant: msg.variant ?? "default",
      });
    },
    [],
  );

  const handleRestoreDefaults = useCallback(async () => {
    setRestoringDefaults(true);
    try {
      await ensureSettingsDefaults();
      didAutoEnsureRef.current = true;
      await loadAll({ silent: true, allowEnsure: false });
      onToast({ title: t("page.restoreDefaultsSuccess"), variant: "success" });
    } catch (err) {
      console.error("[SettingsPage] Failed to restore defaults:", err);
      onToast({
        title: t("page.restoreDefaultsError"),
        description: describeAppError(err).title,
        variant: "destructive",
      });
    } finally {
      setRestoringDefaults(false);
    }
  }, [loadAll, onToast, t]);

  const emptyState = (
    <div className="flex flex-col items-start gap-3 rounded-md border border-dashed border-border bg-muted/20 p-4">
      <p className="text-sm text-text-muted">{t("page.noSettings")}</p>
      <p className="text-xs text-text-muted">{t("page.noSettingsHint")}</p>
      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={contentRefreshing || restoringDefaults}
          onClick={() => void loadAll({ allowEnsure: true })}
        >
          {t("page.retry")}
        </Button>
        <Button
          type="button"
          size="sm"
          disabled={contentRefreshing || restoringDefaults}
          onClick={() => void handleRestoreDefaults()}
        >
          {restoringDefaults ? <Loader2 className="mr-2 h-4 w-4 animate-spin" aria-hidden /> : null}
          {t("page.restoreDefaults")}
        </Button>
      </div>
    </div>
  );

  return (
    <ModulePageShell
      icon={Settings}
      title={t("page.title")}
      description={t("page.description")}
      bodyClassName={null}
    >
      <div className={mfLayout.moduleWorkspaceSplit}>
        <div className="shrink-0 border-r border-surface-border py-6 pl-6 pr-2">
          <SettingsCategorySidebar
            tabs={visibleTabs}
            activeTab={activeTab}
            onSelectTab={handleSelectTab}
          />
        </div>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-auto p-6">
          {canManageSettings && error != null ? (
            <ErrorState
              className="shrink-0 py-4"
              error={error}
              title={t("page.loadError")}
              onRetry={() => void loadAll({ allowEnsure: true })}
            />
          ) : null}

          <Card className="shrink-0">
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-lg">
                {t(`tabs.${activeTab}` as "tabs.general")}
                {(contentRefreshing || restoringDefaults) && !initialLoading ? (
                  <Loader2
                    className="h-4 w-4 animate-spin text-text-muted"
                    aria-label={t("page.loading")}
                  />
                ) : null}
              </CardTitle>
            </CardHeader>
            <CardContent className="flex flex-col gap-6">
              {permissionsLoading || (isAdmSettingsTab(activeTab) && initialLoading) ? (
                <LoadingState label={t("page.loading")} className="py-12" />
              ) : (
                <>
                  {activeTab === "profile" ? (
                    <section className="space-y-4">
                      <p className="text-sm leading-relaxed text-text-muted">
                        {t("tabs.profileHint")}
                      </p>
                      <ProfileSettingsPanel />
                    </section>
                  ) : null}

                  {activeTab === "notifications" ? (
                    <section className="space-y-4">
                      <p className="text-sm leading-relaxed text-text-muted">
                        {t("tabs.notificationsHint")}
                      </p>
                      <NotificationsSettingsPanel />
                    </section>
                  ) : null}

                  {activeTab === "general" && canManageSettings ? (
                    <section className="space-y-4">
                      <p className="text-sm leading-relaxed text-text-muted">
                        {t("tabs.generalHint")}
                      </p>
                      <DocumentHeaderSettingsPanel
                        onToast={onToast}
                        onSaved={() => void loadAll({ silent: true, allowEnsure: false })}
                      />
                      {hasStoredSettings ? (
                        <SettingsCatalogSections
                          categories={GENERAL_CATEGORIES}
                          definitions={definitions}
                          currencies={currencies}
                          rolesAllowed={overview?.roles_allowed_to_change ?? []}
                          canEdit={canManageSettings}
                          onSaved={() => void loadAll({ silent: true, allowEnsure: false })}
                          onToast={onToast}
                        />
                      ) : (
                        emptyState
                      )}
                      <SiteRulesPanel onToast={onToast} />
                    </section>
                  ) : null}

                  {activeTab === "security" && canManageSettings ? (
                    <section className="space-y-6">
                      <p className="text-sm leading-relaxed text-text-muted">
                        {t("tabs.securityHint")}
                      </p>
                      <PasswordPolicyPanel />
                      <LockoutPolicyPanel />
                      <PolicyEditorPanel domain="session" onToast={onToast} />
                      <PermissionGate permission={P.LOG_VIEW}>
                        <ActivityLogSettingsSection />
                      </PermissionGate>
                    </section>
                  ) : null}

                  {activeTab === "backup" && canManageSettings ? (
                    <section className="space-y-6">
                      {hasStoredSettings ? (
                        <SettingsCatalogSections
                          categories={BACKUP_CATEGORIES}
                          definitions={definitions}
                          currencies={currencies}
                          rolesAllowed={overview?.roles_allowed_to_change ?? []}
                          canEdit={canManageSettings}
                          onSaved={() => void loadAll({ silent: true, allowEnsure: false })}
                          onToast={onToast}
                        />
                      ) : (
                        emptyState
                      )}
                      <BackupOperationsPanel onToast={onToast} />
                      <DatabaseRecoveryKeyPanel onToast={onToast} />
                      <PolicyEditorPanel domain="backup" onToast={onToast} />
                    </section>
                  ) : null}

                  {activeTab === "integrations" && canManageSettings ? (
                    <section className="space-y-6">
                      <p className="text-sm leading-relaxed text-text-muted">
                        {t("tabs.integrationsHint")}
                      </p>
                      <SyncFeedbackPanel />
                      <LicenseEnforcementPanel />
                      <ConnectionProfilesPanel onToast={onToast} />
                      <DiagnosticsPanel />
                    </section>
                  ) : null}
                </>
              )}
            </CardContent>
          </Card>

          {canManageSettings && (auditLog.length > 0 || auditLogError != null) ? (
            <Card className="shrink-0">
              <CardHeader className="pb-3">
                <CardTitle className="flex items-center gap-2 text-lg">
                  <History className="h-5 w-5" />
                  {t("auditLog.title")}
                </CardTitle>
              </CardHeader>
              <CardContent>
                {auditLogError != null ? (
                  <ErrorState
                    error={auditLogError}
                    title={t("auditLog.loadError")}
                    onRetry={() => void loadAuditLog()}
                  />
                ) : (
                  <Timeline
                    items={auditLog}
                    aria-label={t("auditLog.title")}
                    getEntry={(evt) => {
                      const def = definitions.find((d) => d.key === evt.setting_key_or_domain);
                      const valueChange =
                        def && (evt.old_value_json != null || evt.new_value_json != null)
                          ? `${formatSettingValue(def, evt.old_value_json, tAny)} → ${formatSettingValue(def, evt.new_value_json, tAny)}`
                          : null;
                      return {
                        id: String(evt.id),
                        title: def ? tAny(`keys.${def.key}`) : evt.setting_key_or_domain,
                        description: valueChange ?? evt.change_summary,
                        timestamp: evt.changed_at,
                        ...(evt.changed_by_name ? { actor: evt.changed_by_name } : {}),
                        ...(evt.required_step_up
                          ? {
                              badges: [{ label: t("auditLog.stepUp"), tone: "info" as const }],
                            }
                          : {}),
                      };
                    }}
                  />
                )}
              </CardContent>
            </Card>
          ) : null}
        </div>
      </div>
    </ModulePageShell>
  );
}
