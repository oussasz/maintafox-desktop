/**
 * Échelle d'urgence (DI / OT) — the plant's settings of the shared urgency scale.
 *
 * The five levels (codes, order, colours) are fixed by Maintafox (SYSTEM list `WORK.URGENCY`):
 * no level can be added, removed or deactivated. The plant edits, per level, its French and
 * English labels and its target times (response, completion, escalation threshold), which the
 * request SLA, the work-order planning and the backlog use.
 */

import { Check, Lock, Pencil, X } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { urgencyLevelLabel, urgencyTone } from "@/components/common/urgency/use-urgency-scale";
import {
  ReferenceValueTable,
  ReferenceValueTableBody,
  ReferenceValueTableCell,
  ReferenceValueTableGrid,
  ReferenceValueTableHead,
  ReferenceValueTableHeadCell,
  ReferenceValueTableRow,
} from "@/components/lookups/ReferenceValueTable";
import {
  REF_TABLE_ACTIONS_GROUP_CLASS,
  REF_TABLE_EMPTY_ACTIONS_MARK,
  REF_TABLE_TITLE_CLASS,
  refTableIconButtonClass,
} from "@/components/lookups/reference-table-ui";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { StatusChip } from "@/components/ui/status-chip";
import { usePermissions } from "@/hooks/use-permissions";
import { listUrgencyLevels, updateUrgencyLevel } from "@/services/urgency-scale-service";
import { referenceErrorText } from "@/stores/reference-error-text";
import { refreshUrgencyScale } from "@/stores/urgency-scale-store";
import type { UrgencyLevel } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface Draft {
  label_fr: string;
  label_en: string;
  response: string;
  completion: string;
  escalation: string;
}

function draftOf(level: UrgencyLevel): Draft {
  return {
    label_fr: level.label_fr,
    label_en: level.label_en,
    response: level.response_target_hours?.toString() ?? "",
    completion: level.completion_target_hours?.toString() ?? "",
    escalation: level.escalation_threshold_hours?.toString() ?? "",
  };
}

/** Whole hours > 0, else null. */
function hours(raw: string): number | null {
  const n = Number(raw.trim());
  return Number.isInteger(n) && n > 0 ? n : null;
}

export function UrgencyScaleHost() {
  const { t, i18n } = useTranslation("reference");
  const { can } = usePermissions();
  const canManage = can(P.REF_MANAGE);
  const [rows, setRows] = useState<UrgencyLevel[]>([]);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loadError, setLoadError] = useState<unknown>(null);
  const [editingCode, setEditingCode] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);

  const loadRows = useCallback(async () => {
    setLoading(true);
    setError(null);
    setLoadError(null);
    try {
      setRows(await listUrgencyLevels());
    } catch (err) {
      setLoadError(err);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadRows();
  }, [loadRows]);

  const cancelEdit = () => {
    setEditingCode(null);
    setDraft(null);
  };

  const parsed =
    draft != null
      ? {
          response: hours(draft.response),
          completion: hours(draft.completion),
          escalation: hours(draft.escalation),
        }
      : null;
  const draftValid =
    draft != null &&
    parsed != null &&
    draft.label_fr.trim() !== "" &&
    draft.label_en.trim() !== "" &&
    parsed.response != null &&
    parsed.completion != null &&
    parsed.escalation != null;

  const submit = async (level: UrgencyLevel) => {
    if (!draft || !parsed || !draftValid) return;
    setSaving(true);
    setError(null);
    try {
      await updateUrgencyLevel(level.code, {
        label_fr: draft.label_fr.trim(),
        label_en: draft.label_en.trim(),
        response_target_hours: parsed.response as number,
        completion_target_hours: parsed.completion as number,
        escalation_threshold_hours: parsed.escalation as number,
      });
      cancelEdit();
      await loadRows();
      await refreshUrgencyScale();
    } catch (err) {
      setError(referenceErrorText(err));
    } finally {
      setSaving(false);
    }
  };

  const sourceChip = (level: UrgencyLevel) => {
    if (level.targets_source === "company") {
      return <StatusChip label={t("urgencyScale.sourceCompany")} tone="success" />;
    }
    if (level.targets_source === "di_sla_rule") {
      return (
        <StatusChip
          label={t("urgencyScale.sourceSlaRule")}
          tone="info"
          title={t("urgencyScale.sourceSlaRuleHint")}
        />
      );
    }
    return (
      <StatusChip
        label={t("urgencyScale.sourceStarter")}
        tone="warning"
        title={t("urgencyScale.sourceStarterHint")}
      />
    );
  };

  const hoursInput = (field: "response" | "completion" | "escalation", label: string) => (
    <Input
      type="number"
      inputMode="numeric"
      min={1}
      step={1}
      className="h-7 w-20 text-sm"
      aria-label={label}
      value={draft?.[field] ?? ""}
      onChange={(e) => setDraft((d) => (d ? { ...d, [field]: e.target.value } : d))}
      aria-invalid={draft != null && hours(draft[field]) == null}
    />
  );

  return (
    <ReferenceValueTable
      loading={loading && rows.length === 0}
      error={error}
      loadError={loadError}
      onRetry={() => void loadRows()}
      title={
        <>
          <span className={REF_TABLE_TITLE_CLASS}>{t("urgencyScale.panelTitle")}</span>
          <StatusChip
            label={t("synthetic.liveTable")}
            tone="info"
            title={t("synthetic.liveTableHint")}
          />
        </>
      }
      banner={
        <p className="border-b border-surface-border px-4 py-2 text-xs text-text-muted">
          {t("urgencyScale.hint")}
        </p>
      }
      emptyLabel={t("urgencyScale.empty")}
      showEmpty={rows.length === 0}
      confirm={null}
    >
      <ReferenceValueTableGrid>
        <ReferenceValueTableHead>
          <tr>
            <ReferenceValueTableHeadCell>{t("urgencyScale.colRank")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("urgencyScale.colLevel")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("urgencyScale.colLabelFr")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("urgencyScale.colLabelEn")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("urgencyScale.colResponse")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("urgencyScale.colCompletion")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>
              {t("urgencyScale.colEscalation")}
            </ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell>{t("urgencyScale.colSource")}</ReferenceValueTableHeadCell>
            <ReferenceValueTableHeadCell align="right">
              {t("urgencyScale.colActions")}
            </ReferenceValueTableHeadCell>
          </tr>
        </ReferenceValueTableHead>
        <ReferenceValueTableBody>
          {rows.map((row) => {
            const editing = editingCode === row.code && draft != null;
            return (
              <ReferenceValueTableRow key={row.code} highlighted={editing}>
                <ReferenceValueTableCell>
                  <span className="inline-flex items-center gap-1 font-mono text-xs">
                    <Lock
                      className="h-3 w-3 shrink-0 text-text-muted"
                      aria-label={t("urgencyScale.lockedHint")}
                    />
                    P{row.rank}
                  </span>
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  <StatusChip
                    label={urgencyLevelLabel(row, i18n.language)}
                    tone={urgencyTone(row.code)}
                    dot
                  />
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {editing ? (
                    <Input
                      className="h-7 text-sm"
                      aria-label={t("urgencyScale.colLabelFr")}
                      value={draft.label_fr}
                      onChange={(e) =>
                        setDraft((d) => (d ? { ...d, label_fr: e.target.value } : d))
                      }
                    />
                  ) : (
                    row.label_fr
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {editing ? (
                    <Input
                      className="h-7 text-sm"
                      aria-label={t("urgencyScale.colLabelEn")}
                      value={draft.label_en}
                      onChange={(e) =>
                        setDraft((d) => (d ? { ...d, label_en: e.target.value } : d))
                      }
                    />
                  ) : (
                    row.label_en
                  )}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {editing
                    ? hoursInput("response", t("urgencyScale.colResponse"))
                    : t("urgencyScale.hours", { count: row.response_target_hours ?? 0 })}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {editing
                    ? hoursInput("completion", t("urgencyScale.colCompletion"))
                    : t("urgencyScale.hours", { count: row.completion_target_hours ?? 0 })}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>
                  {editing
                    ? hoursInput("escalation", t("urgencyScale.colEscalation"))
                    : t("urgencyScale.hours", { count: row.escalation_threshold_hours ?? 0 })}
                </ReferenceValueTableCell>
                <ReferenceValueTableCell>{sourceChip(row)}</ReferenceValueTableCell>
                <ReferenceValueTableCell align="right">
                  {!canManage ? (
                    <span className="text-xs text-text-muted">{REF_TABLE_EMPTY_ACTIONS_MARK}</span>
                  ) : editing ? (
                    <div className={REF_TABLE_ACTIONS_GROUP_CLASS}>
                      <Button
                        aria-label={String(t("editor.save"))}
                        title={String(t("editor.save"))}
                        variant="ghost"
                        size="icon"
                        className={refTableIconButtonClass()}
                        onClick={() => void submit(row)}
                        disabled={saving || !draftValid}
                      >
                        <Check className="h-3.5 w-3.5 text-status-success" />
                      </Button>
                      <Button
                        aria-label={String(t("editor.cancel"))}
                        title={String(t("editor.cancel"))}
                        variant="ghost"
                        size="icon"
                        className={refTableIconButtonClass()}
                        onClick={cancelEdit}
                      >
                        <X className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  ) : (
                    <Button
                      variant="ghost"
                      size="icon"
                      className={refTableIconButtonClass()}
                      onClick={() => {
                        setEditingCode(row.code);
                        setDraft(draftOf(row));
                      }}
                      disabled={saving || editingCode != null}
                      aria-label={t("urgencyScale.editLevel", { level: row.label_fr })}
                      title={t("urgencyScale.editLevel", { level: row.label_fr })}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </ReferenceValueTableCell>
              </ReferenceValueTableRow>
            );
          })}
        </ReferenceValueTableBody>
      </ReferenceValueTableGrid>
    </ReferenceValueTable>
  );
}
