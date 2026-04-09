/**
 * DiLookupManagerDialog.tsx
 *
 * Reusable inline lookup-manager modal for DI reference values
 * (failure_mode, production_impact, symptom, etc.).
 *
 * Phase 2 – Sub-phase 04 – Sprint S4.
 */

import { Pencil, Plus, Trash2, X } from "lucide-react";
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
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import {
  createReferenceValue,
  deactivateReferenceValue,
  listReferenceDomains,
  listReferenceSets,
  listReferenceValues,
  updateReferenceValue,
} from "@/services/reference-service";
import type { ReferenceValue } from "@shared/ipc-types";

// ── Suggestion chips per domain ─────────────────────────────────────────────

const SUGGESTION_CHIPS: Record<string, string[]> = {
  "WORK.FAILURE_MODES": [
    "Mechanical wear",
    "Electrical fault",
    "Overheating",
    "Vibration",
    "Corrosion",
    "Leakage",
    "Misalignment",
    "Fatigue crack",
  ],
  production_impact: [
    "Full stoppage",
    "Partial slowdown",
    "Quality degradation",
    "Safety risk",
    "No impact",
    "Deferred output",
  ],
};

// ── Props ───────────────────────────────────────────────────────────────────

interface DiLookupManagerDialogProps {
  open: boolean;
  onClose: () => void;
  domain: string;
  title: string;
  onValueSelected?: (id: number) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiLookupManagerDialog({
  open,
  onClose,
  domain,
  title,
  onValueSelected,
}: DiLookupManagerDialogProps) {
  const { t } = useTranslation("di");

  const [values, setValues] = useState<ReferenceValue[]>([]);
  const [loading, setLoading] = useState(false);
  const [activeSetId, setActiveSetId] = useState<number | null>(null);

  // Form state
  const [editingId, setEditingId] = useState<number | null>(null);
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [saving, setSaving] = useState(false);

  // ── Load domain → set → values ──────────────────────────────────────────

  const loadValues = useCallback(async () => {
    setLoading(true);
    try {
      const domains = await listReferenceDomains();
      const dom = domains.find((d) => d.code === domain);
      if (!dom) return;

      const sets = await listReferenceSets(dom.id);
      // Use the latest published set, or the first available set
      const activeSet =
        sets.find((s) => s.status === "published") ??
        sets.find((s) => s.status === "draft") ??
        sets[0];
      if (!activeSet) return;

      setActiveSetId(activeSet.id);
      const vals = await listReferenceValues(activeSet.id);
      setValues(vals.filter((v) => v.is_active));
    } catch {
      // silently handle — empty list shown
    } finally {
      setLoading(false);
    }
  }, [domain]);

  useEffect(() => {
    if (open) {
      void loadValues();
    }
  }, [open, loadValues]);

  // ── CRUD handlers ───────────────────────────────────────────────────────

  const resetForm = useCallback(() => {
    setEditingId(null);
    setName("");
    setDescription("");
  }, []);

  const handleEdit = useCallback((val: ReferenceValue) => {
    setEditingId(val.id);
    setName(val.label);
    setDescription(val.description ?? "");
  }, []);

  const handleSave = useCallback(async () => {
    if (!name.trim() || activeSetId === null) return;
    setSaving(true);
    try {
      if (editingId !== null) {
        await updateReferenceValue(editingId, {
          label: name.trim(),
          description: description.trim() || null,
        });
      } else {
        // Generate a code from the label
        const code = name
          .trim()
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, "_")
          .replace(/^_|_$/g, "")
          .slice(0, 50);
        await createReferenceValue({
          set_id: activeSetId,
          code,
          label: name.trim(),
          description: description.trim() || null,
        });
      }
      resetForm();
      await loadValues();
    } catch {
      // error handled silently for now
    } finally {
      setSaving(false);
    }
  }, [name, description, editingId, activeSetId, resetForm, loadValues]);

  const handleDelete = useCallback(
    async (valueId: number) => {
      try {
        await deactivateReferenceValue(valueId);
        await loadValues();
        if (editingId === valueId) resetForm();
      } catch {
        // error handled silently
      }
    },
    [editingId, resetForm, loadValues],
  );

  const handleSelect = useCallback(
    (val: ReferenceValue) => {
      onValueSelected?.(val.id);
      onClose();
    },
    [onValueSelected, onClose],
  );

  const handleChipClick = useCallback((chip: string) => {
    setName(chip);
  }, []);

  const chips = SUGGESTION_CHIPS[domain] ?? [];

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        className="max-w-3xl max-h-[85vh] flex flex-col"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
        </DialogHeader>

        <div className="flex flex-1 gap-4 min-h-0 overflow-hidden">
          {/* ── Left pane: items list ──────────────────────────────── */}
          <div className="flex flex-col w-1/2 border rounded-md">
            <div className="px-3 py-2 border-b bg-muted/30">
              <span className="text-sm font-medium">{t("lookup.existingValues")}</span>
            </div>
            <div className="flex-1 overflow-auto p-2 space-y-1">
              {loading ? (
                <p className="text-sm text-text-muted p-2">{t("lookup.loading")}</p>
              ) : values.length === 0 ? (
                <p className="text-sm text-text-muted p-2">{t("lookup.empty")}</p>
              ) : (
                values.map((val) => (
                  <button
                    type="button"
                    key={val.id}
                    className={`flex w-full items-center justify-between p-2 rounded-md text-sm text-left hover:bg-muted/50 cursor-pointer ${
                      editingId === val.id ? "bg-muted" : ""
                    }`}
                    onClick={() => (onValueSelected ? handleSelect(val) : handleEdit(val))}
                  >
                    <div className="flex flex-col min-w-0 flex-1">
                      <span className="truncate font-medium">{val.label}</span>
                      {val.description && (
                        <span className="text-xs text-text-muted truncate">{val.description}</span>
                      )}
                    </div>
                    <div className="flex items-center gap-1 ml-2 shrink-0">
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 w-7 p-0"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleEdit(val);
                        }}
                      >
                        <Pencil className="h-3.5 w-3.5" />
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 w-7 p-0 text-destructive hover:text-destructive"
                        onClick={(e) => {
                          e.stopPropagation();
                          void handleDelete(val.id);
                        }}
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </Button>
                    </div>
                  </button>
                ))
              )}
            </div>
          </div>

          {/* ── Right pane: create/edit form ───────────────────────── */}
          <div className="flex flex-col w-1/2 space-y-4">
            <div className="flex items-center justify-between">
              <span className="text-sm font-medium">
                {editingId !== null ? t("lookup.editTitle") : t("lookup.createTitle")}
              </span>
              {editingId !== null && (
                <Button variant="ghost" size="sm" className="h-7 gap-1" onClick={resetForm}>
                  <Plus className="h-3.5 w-3.5" />
                  {t("lookup.newEntry")}
                </Button>
              )}
            </div>

            {/* Name input */}
            <div className="space-y-1.5">
              <Label htmlFor="lookup-name">
                {t("lookup.name")}
                <span className="text-destructive ml-0.5">*</span>
              </Label>
              <Input
                id="lookup-name"
                value={name}
                onChange={(e) => setName(e.target.value.slice(0, 100))}
                placeholder={t("lookup.namePlaceholder")}
                maxLength={100}
              />
              <p className="text-xs text-text-muted text-right">{name.length}/100</p>
            </div>

            {/* Suggestion chips */}
            {chips.length > 0 && (
              <div className="space-y-1.5">
                <Label>{t("lookup.suggestions")}</Label>
                <div className="flex flex-wrap gap-1.5">
                  {chips.map((chip) => (
                    <Badge
                      key={chip}
                      variant="outline"
                      className="cursor-pointer hover:bg-muted"
                      onClick={() => handleChipClick(chip)}
                    >
                      {chip}
                    </Badge>
                  ))}
                </div>
              </div>
            )}

            {/* Description textarea */}
            <div className="space-y-1.5">
              <Label htmlFor="lookup-desc">{t("lookup.description")}</Label>
              <Textarea
                id="lookup-desc"
                value={description}
                onChange={(e) => setDescription(e.target.value.slice(0, 500))}
                placeholder={t("lookup.descriptionPlaceholder")}
                rows={3}
                maxLength={500}
              />
              <p className="text-xs text-text-muted text-right">{description.length}/500</p>
            </div>

            {/* Save / Cancel */}
            <div className="flex gap-2">
              <Button
                onClick={() => void handleSave()}
                disabled={!name.trim() || saving}
                className="flex-1"
              >
                {saving
                  ? t("lookup.saving")
                  : editingId !== null
                    ? t("lookup.update")
                    : t("lookup.create")}
              </Button>
              {editingId !== null && (
                <Button variant="outline" onClick={resetForm}>
                  {t("lookup.cancel")}
                </Button>
              )}
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            <X className="h-4 w-4 mr-1.5" />
            {t("lookup.close")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
