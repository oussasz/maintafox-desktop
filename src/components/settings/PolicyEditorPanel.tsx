/**
 * PolicyEditorPanel.tsx
 *
 * Full Draft → Test → Activate workflow for governed settings.
 * Side-by-side diff (active vs draft), policy-specific edit forms,
 * test results panel, and change history.
 *
 * Phase 1 – Sub-phase 06 – Sprint S4 (GAP SET-02).
 */

import {
  AlertTriangle,
  CheckCircle2,
  FileText,
  History,
  Loader2,
  Shield,
  Trash2,
  XCircle,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  activatePolicy,
  discardPolicyDraft,
  getPolicySnapshot,
  listPolicySnapshots,
  savePolicyDraft,
  testPolicyDraft,
} from "@/services/settings-service";
import { useSettingsStore } from "@/stores/settings-store";
import type { AppSetting, PolicySnapshot } from "@shared/ipc-types";

// ── Governed setting domains ────────────────────────────────────────────────

/** Map a setting key to its policy domain. Returns null if not governed. */
function getGovernedDomain(key: string): string | null {
  if (key.startsWith("session.") || key.startsWith("auth.")) return "session";
  if (key.startsWith("password.")) return "password";
  if (key.startsWith("backup.schedule") || key.startsWith("backup.policy")) return "backup";
  return null;
}

/** Check if a set of settings contains any governed (high-risk) items. */
export function hasGovernedSettings(settings: AppSetting[]): boolean {
  return settings.some(
    (s) => s.setting_risk === "high" || getGovernedDomain(s.setting_key) !== null,
  );
}

/** Partition settings into direct-apply and governed. */
export function partitionSettings(settings: AppSetting[]): {
  direct: AppSetting[];
  governed: AppSetting[];
} {
  const direct: AppSetting[] = [];
  const governed: AppSetting[] = [];
  for (const s of settings) {
    if (s.setting_risk === "high" || getGovernedDomain(s.setting_key) !== null) {
      governed.push(s);
    } else {
      direct.push(s);
    }
  }
  return { direct, governed };
}

// ── Policy-specific form field definitions ──────────────────────────────────

interface PolicyField {
  key: string;
  type: "number" | "boolean";
}

const POLICY_FIELDS: Record<string, PolicyField[]> = {
  session: [
    { key: "max_session_hours", type: "number" },
    { key: "idle_timeout_minutes", type: "number" },
    { key: "step_up_window_seconds", type: "number" },
  ],
  password: [
    { key: "min_length", type: "number" },
    { key: "require_uppercase", type: "boolean" },
    { key: "require_number", type: "boolean" },
    { key: "max_age_days", type: "number" },
    { key: "history_count", type: "number" },
  ],
  backup: [
    { key: "schedule_cron", type: "number" },
    { key: "retention_days", type: "number" },
    { key: "include_photos", type: "boolean" },
    { key: "compression_level", type: "number" },
  ],
};

/** Domains that require step-up auth to activate. */
const SECURITY_DOMAINS = new Set(["session", "password"]);

// ── Helpers ─────────────────────────────────────────────────────────────────

function parseJson(json: string): Record<string, unknown> {
  try {
    return JSON.parse(json) as Record<string, unknown>;
  } catch {
    return {};
  }
}

// ── Props ───────────────────────────────────────────────────────────────────

interface PolicyEditorPanelProps {
  settings: AppSetting[];
  onToast: (msg: {
    title: string;
    description?: string;
    variant?: "default" | "destructive" | "success";
  }) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function PolicyEditorPanel({ settings, onToast }: PolicyEditorPanelProps) {
  const { t } = useTranslation("settings");

  const {
    draftPolicy,
    testResults,
    policyOperationLoading,
    setDraftPolicy,
    setTestResults,
    setPolicyOperationLoading,
  } = useSettingsStore();

  const [activeSnapshot, setActiveSnapshot] = useState<PolicySnapshot | null>(null);
  const [history, setHistory] = useState<PolicySnapshot[]>([]);
  const [loading, setLoading] = useState(false);
  const [editing, setEditing] = useState(false);
  const [discardOpen, setDiscardOpen] = useState(false);
  const [draftValues, setDraftValues] = useState<Record<string, unknown>>({});

  // Determine which policy domain these settings belong to
  const domain = useMemo(
    () => settings.map((s) => getGovernedDomain(s.setting_key)).find(Boolean) ?? "session",
    [settings],
  );

  const fields = POLICY_FIELDS[domain] ?? [];
  const requiresStepUp = SECURITY_DOMAINS.has(domain);

  // ── Data loading ──────────────────────────────────────────────────────

  const loadData = useCallback(async () => {
    setLoading(true);
    try {
      const [snap, snapshots] = await Promise.all([
        getPolicySnapshot(domain),
        listPolicySnapshots(domain).catch(() => [] as PolicySnapshot[]),
      ]);
      setActiveSnapshot(snap);
      setHistory(snapshots);

      // Look for existing draft in history
      const existingDraft = snapshots.find((s) => !s.is_active && !s.activated_at);
      setDraftPolicy(existingDraft ?? null);
    } catch {
      // snapshot may not exist yet
    } finally {
      setLoading(false);
    }
  }, [domain, setDraftPolicy]);

  useEffect(() => {
    void loadData();
  }, [loadData]);

  const activeValues = useMemo(
    () => parseJson(activeSnapshot?.snapshot_json ?? "{}"),
    [activeSnapshot],
  );
  const draftSnapshotValues = useMemo(
    () => parseJson(draftPolicy?.snapshot_json ?? "{}"),
    [draftPolicy],
  );

  const hasActiveSnapshot = activeSnapshot?.is_active === true;

  // ── Draft editing ─────────────────────────────────────────────────────

  const handleEditDraft = useCallback(() => {
    // Pre-fill with draft values if draft exists, otherwise from active
    const source = Object.keys(draftSnapshotValues).length > 0 ? draftSnapshotValues : activeValues;
    setDraftValues({ ...source });
    setEditing(true);
  }, [activeValues, draftSnapshotValues]);

  const handleCancelEdit = useCallback(() => {
    setEditing(false);
    setDraftValues({});
  }, []);

  const handleSaveDraft = useCallback(async () => {
    setPolicyOperationLoading(true);
    try {
      const saved = await savePolicyDraft({
        domain,
        snapshot_json: JSON.stringify(draftValues),
      });
      setDraftPolicy(saved);
      setEditing(false);
      setTestResults(null);
      onToast({
        title: t("policy.draftSaved"),
        variant: "success",
      });
      void loadData();
    } catch (err) {
      onToast({
        title: t("policy.draftSaveError"),
        description: String(err),
        variant: "destructive",
      });
    } finally {
      setPolicyOperationLoading(false);
    }
  }, [
    domain,
    draftValues,
    loadData,
    onToast,
    setDraftPolicy,
    setPolicyOperationLoading,
    setTestResults,
    t,
  ]);

  // ── Test draft ────────────────────────────────────────────────────────

  const handleTestDraft = useCallback(async () => {
    setPolicyOperationLoading(true);
    try {
      const results = await testPolicyDraft(domain);
      setTestResults(results);
      const hasFailures = results.some((r) => r.severity === "fail");
      onToast({
        title: hasFailures ? t("policy.testFailed") : t("policy.testPassed"),
        variant: hasFailures ? "destructive" : "success",
      });
    } catch (err) {
      onToast({
        title: t("policy.testError"),
        description: String(err),
        variant: "destructive",
      });
    } finally {
      setPolicyOperationLoading(false);
    }
  }, [domain, onToast, setPolicyOperationLoading, setTestResults, t]);

  // ── Activate ──────────────────────────────────────────────────────────

  const handleActivate = useCallback(async () => {
    if (!draftPolicy) return;
    setPolicyOperationLoading(true);
    try {
      await activatePolicy({
        domain,
        snapshot_id: draftPolicy.id,
      });
      setDraftPolicy(null);
      setTestResults(null);
      onToast({
        title: t("policy.activated"),
        variant: "success",
      });
      void loadData();
    } catch (err) {
      const msg = String(err);
      if (msg.includes("STEP_UP_REQUIRED")) {
        onToast({
          title: t("policy.stepUpRequired"),
          variant: "destructive",
        });
      } else {
        onToast({
          title: t("policy.activateError"),
          description: msg,
          variant: "destructive",
        });
      }
    } finally {
      setPolicyOperationLoading(false);
    }
  }, [
    domain,
    draftPolicy,
    loadData,
    onToast,
    setDraftPolicy,
    setPolicyOperationLoading,
    setTestResults,
    t,
  ]);

  // ── Discard ───────────────────────────────────────────────────────────

  const handleDiscard = useCallback(async () => {
    setPolicyOperationLoading(true);
    try {
      await discardPolicyDraft(domain);
      setDraftPolicy(null);
      setTestResults(null);
      setDiscardOpen(false);
      onToast({
        title: t("policy.discarded"),
        variant: "success",
      });
      void loadData();
    } catch (err) {
      onToast({
        title: t("policy.discardError"),
        description: String(err),
        variant: "destructive",
      });
    } finally {
      setPolicyOperationLoading(false);
    }
  }, [domain, loadData, onToast, setDraftPolicy, setPolicyOperationLoading, setTestResults, t]);

  // ── Form field update handler ─────────────────────────────────────────

  const updateDraftField = useCallback((key: string, value: unknown) => {
    setDraftValues((prev) => ({ ...prev, [key]: value }));
  }, []);

  // ── Diff detection ────────────────────────────────────────────────────

  const isValueChanged = useCallback(
    (key: string, draftVal: unknown): boolean => {
      return String(activeValues[key]) !== String(draftVal);
    },
    [activeValues],
  );

  // ── Render ────────────────────────────────────────────────────────────

  if (loading) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-muted">
        <p>{t("page.loading")}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-1 flex-col gap-4 overflow-auto">
      {/* Status banner */}
      <div className="flex items-center gap-3 rounded-md border bg-muted/30 p-3">
        <Shield className="h-5 w-5 shrink-0 text-primary" />
        <div className="flex-1">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium">{t("policy.governedTitle")}</span>
            {hasActiveSnapshot && (
              <Badge variant="outline" className="bg-green-50 text-xs text-green-700">
                {t("policy.active")}
              </Badge>
            )}
            {draftPolicy && (
              <Badge className="bg-amber-100 text-xs text-amber-800">
                {t("policy.draftPending")}
              </Badge>
            )}
            {requiresStepUp && (
              <Badge variant="outline" className="text-xs">
                {t("policy.requiresStepUp")}
              </Badge>
            )}
          </div>
          <p className="mt-0.5 text-xs text-text-muted">{t("policy.governedDescription")}</p>
        </div>
      </div>

      {/* Side-by-side: Active vs Draft */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {/* Active configuration */}
        <div className="space-y-3">
          <h3 className="text-sm font-medium">{t("policy.activeConfig")}</h3>
          {hasActiveSnapshot ? (
            <div className="rounded-md border">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-left text-text-muted">
                    <th className="px-3 py-2 font-medium">{t("table.key")}</th>
                    <th className="px-3 py-2 font-medium">{t("table.value")}</th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(activeValues).map(([key, val]) => (
                    <tr key={key} className="border-b last:border-0">
                      <td className="px-3 py-2 font-medium text-text-primary">
                        {t(`policy.fields.${key}` as never, {
                          defaultValue: key,
                        })}
                      </td>
                      <td className="px-3 py-2">
                        <code className="rounded bg-muted px-2 py-0.5 text-xs">{String(val)}</code>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {activeSnapshot && (
                <p className="border-t px-3 py-2 text-xs text-text-muted">
                  {t("policy.version", {
                    version: activeSnapshot.version_no,
                  })}
                  {activeSnapshot.activated_at &&
                    ` — ${t("policy.activatedAt", {
                      date: new Date(activeSnapshot.activated_at).toLocaleString(),
                    })}`}
                </p>
              )}
            </div>
          ) : (
            <div className="flex flex-col items-center gap-2 rounded-md border border-dashed p-6 text-text-muted">
              <FileText className="h-8 w-8" />
              <p className="text-sm">{t("policy.noSnapshot")}</p>
            </div>
          )}
        </div>

        {/* Draft column */}
        <div className="space-y-3">
          <h3 className="text-sm font-medium">{t("policy.draftConfig")}</h3>
          {editing ? (
            /* Edit form */
            <div className="space-y-3 rounded-md border border-amber-200 bg-amber-50/30 p-3">
              {fields.map((field) => {
                const val = draftValues[field.key];
                return (
                  <div key={field.key} className="flex items-center justify-between gap-2">
                    <Label htmlFor={`draft-${field.key}`} className="text-sm">
                      {t(`policy.fields.${field.key}` as never, {
                        defaultValue: field.key,
                      })}
                    </Label>
                    {field.type === "boolean" ? (
                      <Checkbox
                        id={`draft-${field.key}`}
                        checked={Boolean(val)}
                        onCheckedChange={(checked) => updateDraftField(field.key, checked === true)}
                      />
                    ) : (
                      <Input
                        id={`draft-${field.key}`}
                        type="number"
                        className="w-28"
                        value={val !== undefined ? String(val) : ""}
                        onChange={(e) => updateDraftField(field.key, Number(e.target.value))}
                      />
                    )}
                  </div>
                );
              })}
              <div className="flex gap-2 pt-2">
                <Button size="sm" onClick={handleSaveDraft} disabled={policyOperationLoading}>
                  {policyOperationLoading && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
                  {t("editor.save")}
                </Button>
                <Button size="sm" variant="outline" onClick={handleCancelEdit}>
                  {t("editor.cancel")}
                </Button>
              </div>
            </div>
          ) : draftPolicy ? (
            /* Read-only draft diff */
            <div className="rounded-md border border-amber-200">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-left text-text-muted">
                    <th className="px-3 py-2 font-medium">{t("table.key")}</th>
                    <th className="px-3 py-2 font-medium">{t("table.value")}</th>
                  </tr>
                </thead>
                <tbody>
                  {Object.entries(draftSnapshotValues).map(([key, val]) => {
                    const changed = isValueChanged(key, val);
                    return (
                      <tr
                        key={key}
                        className={`border-b last:border-0 ${changed ? "bg-amber-50" : ""}`}
                      >
                        <td className="px-3 py-2 font-medium text-text-primary">
                          {t(`policy.fields.${key}` as never, {
                            defaultValue: key,
                          })}
                        </td>
                        <td className="px-3 py-2">
                          <code
                            className={`rounded px-2 py-0.5 text-xs ${changed ? "bg-amber-200 text-amber-900" : "bg-muted"}`}
                          >
                            {String(val)}
                          </code>
                          {changed && (
                            <span className="ml-1 text-xs text-amber-600">
                              ← {String(activeValues[key] ?? "–")}
                            </span>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <p className="border-t px-3 py-2 text-xs text-text-muted">
                {t("policy.version", {
                  version: draftPolicy.version_no,
                })}
              </p>
            </div>
          ) : (
            <div className="flex flex-col items-center gap-2 rounded-md border border-dashed p-6 text-text-muted">
              <FileText className="h-8 w-8" />
              <p className="text-sm">{t("policy.noDraft")}</p>
            </div>
          )}
        </div>
      </div>

      {/* Workflow buttons */}
      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          onClick={handleEditDraft}
          disabled={policyOperationLoading || editing}
        >
          {t("policy.editDraft")}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={handleTestDraft}
          disabled={policyOperationLoading || !draftPolicy}
        >
          {policyOperationLoading && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
          {t("policy.testDraft")}
        </Button>
        <Button
          variant="outline"
          size="sm"
          onClick={handleActivate}
          disabled={policyOperationLoading || !draftPolicy}
        >
          {requiresStepUp && <Shield className="mr-1 h-3 w-3" />}
          {t("policy.activate")}
        </Button>
        {draftPolicy && (
          <Button
            variant="outline"
            size="sm"
            className="text-destructive"
            onClick={() => setDiscardOpen(true)}
            disabled={policyOperationLoading}
          >
            <Trash2 className="mr-1 h-3 w-3" />
            {t("policy.discard")}
          </Button>
        )}
      </div>

      {/* Test results */}
      {testResults && testResults.length > 0 && (
        <div className="space-y-2">
          <h3 className="text-sm font-medium">{t("policy.testResults")}</h3>
          <div className="space-y-1 rounded-md border p-3">
            {testResults.map((result) => (
              <div key={result.rule_name} className="flex items-start gap-2 text-sm">
                {result.severity === "pass" && (
                  <CheckCircle2 className="mt-0.5 h-4 w-4 shrink-0 text-green-600" />
                )}
                {result.severity === "warn" && (
                  <AlertTriangle className="mt-0.5 h-4 w-4 shrink-0 text-amber-500" />
                )}
                {result.severity === "fail" && (
                  <XCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                )}
                <span>{result.message}</span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Current setting values (read-only) */}
      {settings.length > 0 && (
        <div className="space-y-3">
          <h3 className="text-sm font-medium">{t("policy.currentValues")}</h3>
          <div className="rounded-md border">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left text-text-muted">
                  <th className="px-3 py-2 font-medium">{t("table.key")}</th>
                  <th className="px-3 py-2 font-medium">{t("table.value")}</th>
                  <th className="px-3 py-2 font-medium">{t("table.risk")}</th>
                  <th className="px-3 py-2 font-medium">{t("table.status")}</th>
                </tr>
              </thead>
              <tbody>
                {settings.map((s) => {
                  let parsed: unknown;
                  try {
                    parsed = JSON.parse(s.setting_value_json);
                  } catch {
                    parsed = s.setting_value_json;
                  }
                  return (
                    <tr key={s.id} className="border-b last:border-0">
                      <td className="px-3 py-2">
                        <span className="font-medium text-text-primary">
                          {t(`keys.${s.setting_key}` as never, {
                            defaultValue: s.setting_key,
                          })}
                        </span>
                      </td>
                      <td className="px-3 py-2">
                        <code className="rounded bg-muted px-2 py-0.5 text-xs">
                          {String(parsed)}
                        </code>
                      </td>
                      <td className="px-3 py-2">
                        <Badge
                          variant={s.setting_risk === "high" ? "destructive" : "secondary"}
                          className="text-xs"
                        >
                          {t(`risk.${s.setting_risk}` as "risk.low")}
                        </Badge>
                      </td>
                      <td className="px-3 py-2">
                        <Badge variant="outline" className="text-xs">
                          {t(`validation.${s.validation_status}` as "validation.valid")}
                        </Badge>
                        {s.validation_status === "draft" && (
                          <Badge className="ml-1 bg-amber-100 text-xs text-amber-800">
                            {t("policy.draftPending")}
                          </Badge>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>
      )}

      {/* Change history */}
      {history.length > 0 && (
        <div className="space-y-3">
          <h3 className="flex items-center gap-2 text-sm font-medium">
            <History className="h-4 w-4" />
            {t("policy.changeHistory")}
          </h3>
          <div className="divide-y divide-border rounded-md border">
            {history.map((snap) => (
              <div key={snap.id} className="flex items-center justify-between px-3 py-2 text-sm">
                <div className="flex items-center gap-2">
                  <Badge variant={snap.is_active ? "default" : "outline"} className="text-xs">
                    v{snap.version_no}
                  </Badge>
                  <span>
                    {snap.is_active
                      ? t("policy.historyActive")
                      : snap.activated_at
                        ? t("policy.historySuperseded")
                        : t("policy.historyDraft")}
                  </span>
                </div>
                <span className="text-xs text-text-muted">
                  {snap.activated_at ? new Date(snap.activated_at).toLocaleString() : "—"}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Discard confirmation dialog */}
      <Dialog open={discardOpen} onOpenChange={setDiscardOpen}>
        <DialogContent className="max-w-sm">
          <DialogHeader>
            <DialogTitle>{t("policy.discardConfirmTitle")}</DialogTitle>
            <DialogDescription>{t("policy.discardConfirmDescription")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" size="sm" onClick={() => setDiscardOpen(false)}>
              {t("editor.cancel")}
            </Button>
            <Button
              variant="destructive"
              size="sm"
              onClick={handleDiscard}
              disabled={policyOperationLoading}
            >
              {policyOperationLoading && <Loader2 className="mr-1 h-3 w-3 animate-spin" />}
              {t("policy.discard")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
