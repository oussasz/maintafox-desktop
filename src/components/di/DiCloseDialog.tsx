/**
 * Close Request dialog — governed disposition close (no WO).
 */

import { AlertTriangle, Loader2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { ReferenceCombobox } from "@/components/reference/ReferenceCombobox";
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
import { Separator } from "@/components/ui/separator";
import { Textarea } from "@/components/ui/textarea";
import { useSession } from "@/hooks/use-session";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { listDis } from "@/services/di-service";
import { useDiReviewStore } from "@/stores/di-review-store";
import { useDiStore } from "@/stores/di-store";
import type { InterventionRequest } from "@shared/ipc-types";

const CONVERT_ONLY_DISPOSITIONS = ["converted_to_wo"] as const;

const URGENCY_STYLE: Record<string, string> = {
  critical: "bg-red-100 text-red-700",
  high: "bg-orange-100 text-orange-800",
  medium: "bg-yellow-100 text-yellow-800",
  low: "bg-green-100 text-green-800",
};

function RelatedDiPicker({
  valueCode,
  resolvedId,
  excludeDiId,
  onChange,
  error,
}: {
  valueCode: string;
  resolvedId: number | null;
  excludeDiId: number;
  onChange: (code: string, id: number | null) => void;
  error?: string | null;
}) {
  const { t } = useTranslation("di");
  const [query, setQuery] = useState(valueCode);
  const debouncedQuery = useDebouncedValue(query, 300);
  const [results, setResults] = useState<InterventionRequest[]>([]);
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    setQuery(valueCode);
  }, [valueCode]);

  useEffect(() => {
    const q = debouncedQuery.trim();
    if (q.length < 1) {
      setResults([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    void listDis({ search: q, limit: 10, offset: 0 })
      .then((page) => {
        if (cancelled) return;
        setResults(page.items.filter((item) => item.id !== excludeDiId));
      })
      .catch(() => {
        if (!cancelled) setResults([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [debouncedQuery, excludeDiId]);

  const exactMatch = results.find((item) => item.code.toLowerCase() === query.trim().toLowerCase());

  return (
    <div className="space-y-1.5">
      <Label htmlFor="close-related-di">{t("close.relatedDi")}</Label>
      <Input
        id="close-related-di"
        value={query}
        onChange={(e) => {
          const next = e.target.value;
          setQuery(next);
          onChange(next, null);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        placeholder={t("close.relatedDiPlaceholder")}
        autoComplete="off"
        className={error ? "border-red-500" : ""}
      />
      {resolvedId != null && valueCode.trim() && (
        <p className="text-[11px] text-muted-foreground">
          {t("close.relatedDiResolved", { code: valueCode.trim() })}
        </p>
      )}
      {open && query.trim().length > 0 && (
        <div className="rounded-md border bg-popover shadow-sm max-h-40 overflow-y-auto">
          {loading && (
            <p className="px-3 py-2 text-xs text-muted-foreground">
              {t("close.relatedDiSearching")}
            </p>
          )}
          {!loading && results.length === 0 && (
            <p className="px-3 py-2 text-xs text-muted-foreground">
              {t("close.relatedDiNoResults")}
            </p>
          )}
          {results.map((item) => (
            <button
              key={item.id}
              type="button"
              className="flex w-full items-center gap-2 px-3 py-2 text-left text-xs hover:bg-muted/60"
              onClick={() => {
                onChange(item.code, item.id);
                setQuery(item.code);
                setOpen(false);
              }}
            >
              <span className="font-mono font-medium">{item.code}</span>
              <span className="truncate text-muted-foreground">{item.title}</span>
            </button>
          ))}
        </div>
      )}
      {error && <p className="text-[11px] text-red-600">{error}</p>}
      {exactMatch && resolvedId == null && (
        <Button
          type="button"
          size="sm"
          variant="outline"
          className="h-7 text-xs"
          onClick={() => {
            onChange(exactMatch.code, exactMatch.id);
            setQuery(exactMatch.code);
            setOpen(false);
          }}
        >
          {t("close.relatedDiUseMatch", { code: exactMatch.code })}
        </Button>
      )}
    </div>
  );
}

export function DiCloseDialog() {
  const { t } = useTranslation("di");
  const di = useDiReviewStore((s) => s.rejectionDi);
  const closeRejection = useDiReviewStore((s) => s.closeRejection);
  const closeWithDisposition = useDiReviewStore((s) => s.closeWithDisposition);
  const saving = useDiReviewStore((s) => s.saving);
  const storeError = useDiReviewStore((s) => s.error);
  const loadDis = useDiStore((s) => s.loadDis);
  const { info } = useSession();

  const [disposition, setDisposition] = useState<string | null>(null);
  const [notes, setNotes] = useState("");
  const [relatedDiCode, setRelatedDiCode] = useState("");
  const [relatedDiId, setRelatedDiId] = useState<number | null>(null);
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [touched, setTouched] = useState(false);

  const open = di !== null;
  const needsRelated = disposition === "duplicate";
  const needsNotes = disposition === "other";

  const resetForm = useCallback(() => {
    setDisposition(null);
    setNotes("");
    setRelatedDiCode("");
    setRelatedDiId(null);
    setFieldError(null);
    setTouched(false);
  }, []);

  const handleClose = useCallback(() => {
    resetForm();
    closeRejection();
  }, [closeRejection, resetForm]);

  const handleSubmit = useCallback(async () => {
    setTouched(true);
    if (!di || !disposition) return;
    if (needsNotes && !notes.trim()) {
      setFieldError(t("close.notesRequired"));
      return;
    }
    if (needsRelated && relatedDiId == null) {
      setFieldError(t("close.relatedRequired"));
      return;
    }
    setFieldError(null);
    try {
      await closeWithDisposition({
        di_id: di.id,
        actor_id: info?.user_id ?? 0,
        expected_row_version: di.row_version,
        disposition_code: disposition,
        notes: notes.trim() || null,
        related_di_id: needsRelated ? relatedDiId : null,
      });
      void loadDis();
      resetForm();
      closeRejection();
    } catch {
      // storeError surfaced below
    }
  }, [
    di,
    disposition,
    needsNotes,
    needsRelated,
    notes,
    relatedDiId,
    closeWithDisposition,
    info,
    loadDis,
    closeRejection,
    resetForm,
    t,
  ]);

  if (!di) return null;

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && handleClose()}>
      <DialogContent className="max-w-lg" onPointerDownOutside={(e) => e.preventDefault()}>
        <DialogHeader>
          <DialogTitle className="text-lg font-bold">{t("close.title")}</DialogTitle>
        </DialogHeader>

        <Separator />

        <div className="space-y-4 py-2">
          <div className="flex items-center gap-2 text-sm">
            <span className="font-mono text-muted-foreground">{di.code}</span>
            <span className="font-semibold truncate">{di.title}</span>
            <Badge
              variant="outline"
              className={`text-[10px] border-0 ml-auto ${URGENCY_STYLE[di.reported_urgency] ?? ""}`}
            >
              {t(`priority.${di.reported_urgency}`)}
            </Badge>
          </div>

          <div className="space-y-1.5">
            <Label>
              {t("close.disposition")} <span className="text-red-500">*</span>
            </Label>
            <ReferenceCombobox
              referenceType="di.disposition"
              value={disposition}
              onChange={setDisposition}
              excludeCodes={CONVERT_ONLY_DISPOSITIONS}
              allowCreate={false}
            />
          </div>

          {needsRelated && (
            <RelatedDiPicker
              valueCode={relatedDiCode}
              resolvedId={relatedDiId}
              excludeDiId={di.id}
              error={
                touched && needsRelated && relatedDiId == null ? t("close.relatedRequired") : null
              }
              onChange={(code, id) => {
                setRelatedDiCode(code);
                setRelatedDiId(id);
              }}
            />
          )}

          <div className="space-y-1.5">
            <Label htmlFor="close-notes">
              {t("close.notes")}
              {needsNotes && <span className="text-red-500"> *</span>}
            </Label>
            <Textarea
              id="close-notes"
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              rows={3}
              maxLength={2000}
              className={touched && needsNotes && !notes.trim() ? "border-red-500" : ""}
            />
          </div>

          <div className="flex items-center gap-2 rounded-md bg-amber-50 border border-amber-200 px-3 py-2 text-xs text-amber-800">
            <AlertTriangle className="h-4 w-4 shrink-0" />
            <span>{t("close.warning")}</span>
          </div>

          {(fieldError ?? storeError) && (
            <p className="text-sm text-destructive">{fieldError ?? storeError}</p>
          )}
        </div>

        <Separator />

        <DialogFooter className="flex items-center justify-end gap-2">
          <Button variant="outline" size="sm" onClick={handleClose} disabled={saving}>
            {t("form.cancel")}
          </Button>
          <Button
            size="sm"
            variant="destructive"
            onClick={() => void handleSubmit()}
            disabled={saving || !disposition}
            className="gap-1.5"
          >
            {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {saving ? t("close.saving") : t("close.submit")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
