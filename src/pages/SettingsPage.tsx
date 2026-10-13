import { History, Loader2, Settings } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useSearchParams } from "react-router-dom";

import { PermissionGate } from "@/components/PermissionGate";
import { PasswordPolicyPanel } from "@/components/admin/PasswordPolicyPanel";
import { DiagnosticsPanel } from "@/components/diagnostics/DiagnosticsPanel";
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
  SETTINGS_TABS,
  SettingsCategorySidebar,
  type SettingsTabId,
} from "@/components/settings/SettingsCategorySidebar";
import { SettingsValueEditor } from "@/components/settings/SettingsValueEditor";
import { SyncFeedbackPanel } from "@/components/sync/SyncFeedbackPanel";
import { Timeline } from "@/components/timeline";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Toast,
  ToastClose,
  ToastDescription,
  ToastProvider,
  ToastTitle,
  ToastViewport,
} from "@/components/ui/toast";
import { mfLayout } from "@/design-system/tokens";
import { usePermissions } from "@/hooks/use-permissions";
import { useToast } from "@/hooks/use-toast";
import { i18n } from "@/i18n";
import {
  ensureSettingsDefaults,
  listAllSettings,
  listSettingChangeEvents,
} from "@/services/settings-service";
import type { AppSetting, SettingsChangeEvent } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

const GENERAL_CATEGORIES = ["localization", "appearance", "system", "reliability"] as const;
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

function filterByCategories(settings: AppSetting[], categories: readonly string[]): AppSetting[] {
  const allowed = new Set(categories);
  return settings.filter((s) => allowed.has(s.category));
}

export function SettingsPage() {
  const { t } = useTranslation("settings");
  const { toasts, toast, dismiss } = useToast();
  const { can, isLoading: permissionsLoading } = usePermissions();
  const canManageSettings = can(P.ADM_SETTINGS);
  const [searchParams, setSearchParams] = useSearchParams();

  const [activeTab, setActiveTab] = useState<SettingsTabId>(() =>
    parseSettingsTab(searchParams.get("tab")),
  );
  const [allSettings, setAllSettings] = useState<AppSetting[]>([]);
  const [auditLog, setAuditLog] = useState<SettingsChangeEvent[]>([]);
  const [initialLoading, setInitialLoading] = useState(true);
  const [contentRefreshing, setContentRefreshing] = useState(false);
  const [restoringDefaults, setRestoringDefaults] = useState(false);
  const [error, setError] = useState<string | null>(null);
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

  const generalSettings = useMemo(
    () => filterByCategories(allSettings, GENERAL_CATEGORIES),
    [allSettings],
  );
  const backupRetentionSettings = useMemo(
    () => filterByCategories(allSettings, BACKUP_CATEGORIES),
    [allSettings],
  );

  const loadAll = useCallback(async (opts?: { silent?: boolean; allowEnsure?: boolean }) => {
    const silent = opts?.silent === true;
    const allowEnsure = opts?.allowEnsure !== false;
    if (!silent) setContentRefreshing(true);
    try {
      let settings = await listAllSettings();
      const events = await listSettingChangeEvents(20).catch(() => [] as SettingsChangeEvent[]);

      if (settings.length === 0 && allowEnsure && !didAutoEnsureRef.current) {
        didAutoEnsureRef.current = true;
        await ensureSettingsDefaults();
        settings = await listAllSettings();
      }

      setAllSettings(settings);
      setAuditLog(events);
      setError(null);
    } catch (err) {
      console.error("[SettingsPage] Failed to load settings:", err);
      setError("loadError");
    } finally {
      setInitialLoading(false);
      setContentRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void i18n.loadNamespaces(["settings", "admin", "activity"]);
  }, []);

  useEffect(() => {
    if (permissionsLoading) return;
    if (!canManageSettings) {
      setInitialLoading(false);
      setContentRefreshing(false);
      setError(null);
      setAllSettings([]);
      setAuditLog([]);
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
      toast({
        title: msg.title,
        ...(msg.description !== undefined ? { description: msg.description } : {}),
        variant:
          msg.variant === "success"
            ? "success"
            : msg.variant === "destructive"
              ? "destructive"
              : "default",
      });
    },
    [toast],
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
        description: String(err),
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
    <ToastProvider>
      <div className={mfLayout.moduleRoot}>
        <div className={mfLayout.moduleHeader}>
          <div className={mfLayout.moduleTitleRow}>
            <Settings className={mfLayout.moduleHeaderIcon} />
            <div>
              <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
              <p className="text-sm text-text-muted">{t("page.description")}</p>
            </div>
          </div>
        </div>

        <div className={mfLayout.moduleWorkspaceSplit}>
          <div className="shrink-0 border-r border-surface-border py-6 pl-6 pr-2">
            <SettingsCategorySidebar
              tabs={visibleTabs}
              activeTab={activeTab}
              onSelectTab={handleSelectTab}
            />
          </div>

          <div className="flex min-h-0 min-w-0 flex-1 flex-col gap-6 overflow-auto p-6">
            {canManageSettings && error ? (
              <div className="flex shrink-0 flex-wrap items-center justify-between gap-3 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
                <span>{t("page.loadError")}</span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => void loadAll({ allowEnsure: true })}
                >
                  {t("page.retry")}
                </Button>
              </div>
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
                  <div className="flex flex-col items-center justify-center gap-2 py-12 text-text-muted">
                    <Loader2 className="h-5 w-5 animate-spin" aria-hidden />
                    <p>{t("page.loading")}</p>
                  </div>
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
                        {generalSettings.length > 0 ? (
                          <SettingsValueEditor
                            settings={generalSettings}
                            onSettingSaved={() =>
                              void loadAll({ silent: true, allowEnsure: false })
                            }
                            onToast={onToast}
                          />
                        ) : (
                          emptyState
                        )}
                      </section>
                    ) : null}

                    {activeTab === "security" && canManageSettings ? (
                      <section className="space-y-6">
                        <p className="text-sm leading-relaxed text-text-muted">
                          {t("tabs.securityHint")}
                        </p>
                        <PasswordPolicyPanel />
                        <PolicyEditorPanel domain="session" onToast={onToast} />
                        <PermissionGate permission={P.LOG_VIEW}>
                          <ActivityLogSettingsSection />
                        </PermissionGate>
                      </section>
                    ) : null}

                    {activeTab === "backup" && canManageSettings ? (
                      <section className="space-y-6">
                        {backupRetentionSettings.length > 0 ? (
                          <SettingsValueEditor
                            settings={backupRetentionSettings}
                            onSettingSaved={() =>
                              void loadAll({ silent: true, allowEnsure: false })
                            }
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

            {canManageSettings && auditLog.length > 0 ? (
              <Card className="shrink-0">
                <CardHeader className="pb-3">
                  <CardTitle className="flex items-center gap-2 text-lg">
                    <History className="h-5 w-5" />
                    {t("auditLog.title")}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <Timeline
                    items={auditLog}
                    aria-label={t("auditLog.title")}
                    getEntry={(evt) => ({
                      id: String(evt.id),
                      title: evt.setting_key_or_domain,
                      description: evt.change_summary,
                      timestamp: evt.changed_at,
                      ...(evt.required_step_up
                        ? {
                            badges: [{ label: t("auditLog.stepUp"), tone: "info" as const }],
                          }
                        : {}),
                    })}
                  />
                </CardContent>
              </Card>
            ) : null}
          </div>
        </div>
      </div>

      {toasts.map((tm) => (
        <Toast key={tm.id} variant={tm.variant ?? "default"} onOpenChange={() => dismiss(tm.id)}>
          <div className="grid gap-1">
            <ToastTitle>{tm.title}</ToastTitle>
            {tm.description ? <ToastDescription>{tm.description}</ToastDescription> : null}
          </div>
          <ToastClose />
        </Toast>
      ))}
      <ToastViewport />
    </ToastProvider>
  );
}
