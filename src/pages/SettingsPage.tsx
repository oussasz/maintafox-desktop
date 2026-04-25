import { AlertCircle, History, Settings } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { LicenseEnforcementPanel } from "@/components/license/LicenseEnforcementPanel";
import { PolicyEditorPanel, partitionSettings } from "@/components/settings/PolicyEditorPanel";
import { SettingsCategorySidebar } from "@/components/settings/SettingsCategorySidebar";
import { SettingsValueEditor } from "@/components/settings/SettingsValueEditor";
import { SyncFeedbackPanel } from "@/components/sync/SyncFeedbackPanel";
import { Badge } from "@/components/ui/badge";
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
import { useToast } from "@/hooks/use-toast";
import {
  listSettingChangeEvents,
  listSettingsByCategory,
  listSettingsCategories,
} from "@/services/settings-service";
import type { AppSetting, SettingsChangeEvent } from "@shared/ipc-types";

export function SettingsPage() {
  const { t } = useTranslation("settings");
  const { toasts, toast, dismiss } = useToast();

  const [categories, setCategories] = useState<string[]>([]);
  const [activeCategory, setActiveCategory] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSetting[]>([]);
  const [auditLog, setAuditLog] = useState<SettingsChangeEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const { direct, governed } = useMemo(() => partitionSettings(settings), [settings]);

  // Load categories on mount
  useEffect(() => {
    listSettingsCategories()
      .then((cats) => {
        setCategories(cats);
        if (cats.length > 0) setActiveCategory(cats[0] ?? null);
        setError(null);
      })
      .catch((err) => {
        console.error("[SettingsPage] Failed to load categories:", err);
        setError(t("page.loadError") as string);
      })
      .finally(() => setLoading(false));
  }, [t]);

  // Load settings when active category changes
  useEffect(() => {
    if (!activeCategory) return;
    setLoading(true);
    listSettingsByCategory(activeCategory)
      .then((s) => {
        setSettings(s);
        setError(null);
      })
      .catch(() => setError(t("page.loadError") as string))
      .finally(() => setLoading(false));
  }, [activeCategory, t]);

  // Load audit log on mount
  useEffect(() => {
    listSettingChangeEvents(20)
      .then(setAuditLog)
      .catch(() => {});
  }, []);

  const reloadSettings = useCallback(() => {
    if (!activeCategory) return;
    listSettingsByCategory(activeCategory)
      .then(setSettings)
      .catch(() => {});
    listSettingChangeEvents(20)
      .then(setAuditLog)
      .catch(() => {});
  }, [activeCategory]);

  return (
    <PermissionGate
      permission="adm.settings"
      fallback={
        <div className="flex flex-1 items-center justify-center gap-2 text-text-muted">
          <AlertCircle className="h-5 w-5" />
          <span>{t("permissionDenied")}</span>
        </div>
      }
    >
      <ToastProvider>
        <div className={mfLayout.moduleRoot}>
          {/* Header — aligned with DI / OT module pages */}
          <div className={mfLayout.moduleHeader}>
            <div className={mfLayout.moduleTitleRow}>
              <Settings className={mfLayout.moduleHeaderIcon} />
              <div>
                <h1 className={mfLayout.moduleTitle}>{t("page.title")}</h1>
                <p className="text-sm text-text-muted">{t("page.description")}</p>
              </div>
            </div>
          </div>

          <div className={mfLayout.moduleContent}>
            {/* Error state */}
            {error && (
              <div className="rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
                {error}
              </div>
            )}

            {/* Main layout: sidebar + content */}
            <div className="flex min-h-0 flex-1 gap-6 overflow-hidden">
              <SettingsCategorySidebar
                categories={categories}
                activeCategory={activeCategory}
                onSelect={setActiveCategory}
              />

              <Card className="flex flex-1 flex-col overflow-hidden">
                <CardHeader className="pb-3">
                  <CardTitle className="text-lg">
                    {activeCategory
                      ? t(`categories.${activeCategory}` as "categories.localization")
                      : t("page.title")}
                  </CardTitle>
                </CardHeader>
                <CardContent className="flex flex-1 flex-col gap-6 overflow-auto">
                  {loading ? (
                    <div className="flex flex-1 items-center justify-center text-text-muted">
                      <p>{t("page.loading")}</p>
                    </div>
                  ) : (
                    <>
                      {governed.length > 0 && (
                        <PolicyEditorPanel settings={governed} onToast={toast} />
                      )}
                      {direct.length > 0 && (
                        <SettingsValueEditor
                          settings={direct}
                          onSettingSaved={reloadSettings}
                          onToast={toast}
                        />
                      )}
                    </>
                  )}
                </CardContent>
              </Card>
            </div>

            <SyncFeedbackPanel />
            <LicenseEnforcementPanel />

            {/* Audit log */}
            {auditLog.length > 0 && (
              <Card>
                <CardHeader className="pb-3">
                  <CardTitle className="flex items-center gap-2 text-lg">
                    <History className="h-5 w-5" />
                    {t("auditLog.title")}
                  </CardTitle>
                </CardHeader>
                <CardContent>
                  <div className="divide-y divide-border">
                    {auditLog.map((evt) => (
                      <div key={evt.id} className="flex items-center justify-between py-2 text-sm">
                        <div className="flex flex-col gap-0.5">
                          <span className="font-medium text-text-primary">
                            {evt.setting_key_or_domain}
                          </span>
                          <span className="text-text-muted">{evt.change_summary}</span>
                        </div>
                        <div className="flex items-center gap-2">
                          {evt.required_step_up && (
                            <Badge variant="outline">{t("auditLog.stepUp")}</Badge>
                          )}
                          <span className="text-xs text-text-muted">
                            {new Date(evt.changed_at).toLocaleString()}
                          </span>
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>
            )}
          </div>
        </div>

        {/* Toast notifications */}
        {toasts.map((tm) => (
          <Toast key={tm.id} variant={tm.variant ?? "default"} onOpenChange={() => dismiss(tm.id)}>
            <div className="grid gap-1">
              <ToastTitle>{tm.title}</ToastTitle>
              {tm.description && <ToastDescription>{tm.description}</ToastDescription>}
            </div>
            <ToastClose />
          </Toast>
        ))}
        <ToastViewport />
      </ToastProvider>
    </PermissionGate>
  );
}
