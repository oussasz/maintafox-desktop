/**
 * DiSlaRulesPanel.tsx
 *
 * Admin panel for viewing and editing SLA rules.
 * Gated behind `di.admin` permission.
 *
 * Phase 2 – Sub-phase 04 – Sprint S4.
 */

import { Save } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { listSlaRules, updateSlaRule } from "@/services/di-conversion-service";
import type { DiSlaRule, SlaRuleUpdateInput } from "@shared/ipc-types";

// ── Urgency badge styles ────────────────────────────────────────────────────

const URGENCY_BADGE: Record<string, string> = {
  low: "bg-green-100 text-green-800",
  medium: "bg-yellow-100 text-yellow-800",
  high: "bg-orange-100 text-orange-800",
  critical: "bg-red-100 text-red-700",
};

// ── Props ───────────────────────────────────────────────────────────────────

interface DiSlaRulesPanelProps {
  open: boolean;
  onClose: () => void;
}

// ── Local draft type (tracks unsaved edits per rule) ────────────────────────

interface RuleDraft {
  target_response_hours: number;
  target_resolution_hours: number;
  escalation_threshold_hours: number;
  is_active: boolean;
  dirty: boolean;
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiSlaRulesPanel({ open, onClose }: DiSlaRulesPanelProps) {
  const { t } = useTranslation("di");

  const [rules, setRules] = useState<DiSlaRule[]>([]);
  const [drafts, setDrafts] = useState<Record<number, RuleDraft>>({});
  const [loading, setLoading] = useState(false);
  const [savingId, setSavingId] = useState<number | null>(null);

  // ── Load rules ──────────────────────────────────────────────────────────

  const loadRules = useCallback(async () => {
    setLoading(true);
    try {
      const data = await listSlaRules();
      setRules(data);
      const newDrafts: Record<number, RuleDraft> = {};
      for (const r of data) {
        newDrafts[r.id] = {
          target_response_hours: r.target_response_hours,
          target_resolution_hours: r.target_resolution_hours,
          escalation_threshold_hours: r.escalation_threshold_hours,
          is_active: r.is_active,
          dirty: false,
        };
      }
      setDrafts(newDrafts);
    } catch {
      // silently handle
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (open) void loadRules();
  }, [open, loadRules]);

  // ── Draft mutation ──────────────────────────────────────────────────────

  const updateDraft = useCallback((id: number, patch: Partial<Omit<RuleDraft, "dirty">>) => {
    setDrafts((prev) => {
      const existing = prev[id];
      if (!existing) return prev;
      return { ...prev, [id]: { ...existing, ...patch, dirty: true } };
    });
  }, []);

  // ── Save handler ────────────────────────────────────────────────────────

  const handleSave = useCallback(
    async (rule: DiSlaRule) => {
      const draft = drafts[rule.id];
      if (!draft?.dirty) return;
      setSavingId(rule.id);
      try {
        const input: SlaRuleUpdateInput = {
          id: rule.id,
          name: rule.name,
          urgency_level: rule.urgency_level,
          target_response_hours: draft.target_response_hours,
          target_resolution_hours: draft.target_resolution_hours,
          escalation_threshold_hours: draft.escalation_threshold_hours,
          is_active: draft.is_active,
          ...(rule.origin_type !== null ? { origin_type: rule.origin_type } : {}),
          ...(rule.asset_criticality_class !== null
            ? { asset_criticality_class: rule.asset_criticality_class }
            : {}),
        };
        await updateSlaRule(input);
        await loadRules();
      } catch {
        // silently handle
      } finally {
        setSavingId(null);
      }
    },
    [drafts, loadRules],
  );

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        className="max-w-4xl max-h-[85vh] flex flex-col"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{t("sla.title")}</DialogTitle>
          <p className="text-sm text-text-muted">{t("sla.description")}</p>
        </DialogHeader>

        <div className="flex-1 overflow-auto min-h-0">
          {loading ? (
            <p className="text-sm text-text-muted p-4">{t("sla.loading")}</p>
          ) : rules.length === 0 ? (
            <p className="text-sm text-text-muted p-4">{t("sla.empty")}</p>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b text-left">
                  <th className="px-3 py-2 font-medium">{t("sla.columns.name")}</th>
                  <th className="px-3 py-2 font-medium">{t("sla.columns.urgency")}</th>
                  <th className="px-3 py-2 font-medium">{t("sla.columns.response")}</th>
                  <th className="px-3 py-2 font-medium">{t("sla.columns.resolution")}</th>
                  <th className="px-3 py-2 font-medium">{t("sla.columns.escalation")}</th>
                  <th className="px-3 py-2 font-medium">{t("sla.columns.active")}</th>
                  <th className="px-3 py-2 font-medium" />
                </tr>
              </thead>
              <tbody>
                {rules.map((rule) => {
                  const draft = drafts[rule.id];
                  if (!draft) return null;
                  return (
                    <tr key={rule.id} className="border-b last:border-0">
                      <td className="px-3 py-2 font-medium">{rule.name}</td>
                      <td className="px-3 py-2">
                        <Badge
                          variant="outline"
                          className={`text-[10px] border-0 ${URGENCY_BADGE[rule.urgency_level] ?? ""}`}
                        >
                          {t(`priority.${rule.urgency_level}` as never)}
                        </Badge>
                      </td>
                      <td className="px-3 py-2">
                        <Input
                          type="number"
                          min={0}
                          step={1}
                          className="w-20 h-8 text-xs"
                          value={draft.target_response_hours}
                          onChange={(e) =>
                            updateDraft(rule.id, {
                              target_response_hours: Number(e.target.value) || 0,
                            })
                          }
                        />
                      </td>
                      <td className="px-3 py-2">
                        <Input
                          type="number"
                          min={0}
                          step={1}
                          className="w-20 h-8 text-xs"
                          value={draft.target_resolution_hours}
                          onChange={(e) =>
                            updateDraft(rule.id, {
                              target_resolution_hours: Number(e.target.value) || 0,
                            })
                          }
                        />
                      </td>
                      <td className="px-3 py-2">
                        <Input
                          type="number"
                          min={0}
                          step={1}
                          className="w-20 h-8 text-xs"
                          value={draft.escalation_threshold_hours}
                          onChange={(e) =>
                            updateDraft(rule.id, {
                              escalation_threshold_hours: Number(e.target.value) || 0,
                            })
                          }
                        />
                      </td>
                      <td className="px-3 py-2">
                        <Switch
                          checked={draft.is_active}
                          onCheckedChange={(checked) =>
                            updateDraft(rule.id, { is_active: checked })
                          }
                        />
                      </td>
                      <td className="px-3 py-2">
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-8 gap-1.5"
                          disabled={!draft.dirty || savingId === rule.id}
                          onClick={() => void handleSave(rule)}
                        >
                          <Save className="h-3.5 w-3.5" />
                          {savingId === rule.id ? t("sla.saving") : t("sla.save")}
                        </Button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            {t("sla.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
