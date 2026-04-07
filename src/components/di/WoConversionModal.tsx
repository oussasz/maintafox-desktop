/**
 * WoConversionModal.tsx
 *
 * Confirmation modal for converting a DI into a work order shell.
 * Phase 2 – Sub-phase 04 – File 03 – Sprint S3.
 */

import { CheckCircle2, XCircle, Loader2, AlertCircle } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import {
  Button,
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  Input,
  Label,
  Textarea,
} from "@/components/ui";
import { convertDiToWo } from "@/services/di-conversion-service";
import { toErrorMessage } from "@/utils/errors";
import type { InterventionRequest, WoConversionResult } from "@shared/ipc-types";

// ── Props ─────────────────────────────────────────────────────────────────────

interface WoConversionModalProps {
  di: InterventionRequest;
  onConverted: (result: WoConversionResult) => void;
  onClose: () => void;
}

// ── Checklist item ────────────────────────────────────────────────────────────

interface ChecklistItem {
  label: string;
  passed: boolean;
}

// ── Component ─────────────────────────────────────────────────────────────────

export function WoConversionModal({ di, onConverted, onClose }: WoConversionModalProps) {
  const [stepUpPin, setStepUpPin] = useState("");
  const [conversionNotes, setConversionNotes] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // ── Pre-conversion checklist ────────────────────────────────────────────

  const checklist = useMemo<ChecklistItem[]>(
    () => [
      {
        label: "Actif confirmé",
        passed: di.asset_id != null && di.asset_id > 0,
      },
      {
        label: "Classification définie",
        passed: di.classification_code_id != null,
      },
      {
        label: "Urgence validée",
        passed: di.validated_urgency != null,
      },
    ],
    [di.asset_id, di.classification_code_id, di.validated_urgency],
  );

  const allPassed = useMemo(() => checklist.every((item) => item.passed), [checklist]);

  const canConvert = allPassed && stepUpPin.trim().length > 0 && !saving;

  // ── Convert handler ─────────────────────────────────────────────────────

  const handleConvert = useCallback(async () => {
    if (!canConvert) return;
    setSaving(true);
    setError(null);

    try {
      const trimmed = conversionNotes.trim();
      const result = await convertDiToWo({
        diId: di.id,
        expectedRowVersion: di.row_version,
        ...(trimmed ? { conversionNotes: trimmed } : {}),
      });
      onConverted(result);
    } catch (err) {
      setError(toErrorMessage(err));
    } finally {
      setSaving(false);
    }
  }, [canConvert, di.id, di.row_version, conversionNotes, onConverted]);

  // ── Render ──────────────────────────────────────────────────────────────

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Convertir en ordre de travail</DialogTitle>
          <DialogDescription>
            La DI <span className="font-medium">{di.code}</span> sera convertie en ordre de travail.
            Cette action est irréversible.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-2">
          {/* Checklist */}
          <div className="space-y-2">
            <p className="text-sm font-medium text-text-primary">Prérequis de conversion</p>
            <ul className="space-y-1.5">
              {checklist.map((item) => (
                <li key={item.label} className="flex items-center gap-2 text-sm">
                  {item.passed ? (
                    <CheckCircle2 className="h-4 w-4 text-green-600" />
                  ) : (
                    <XCircle className="h-4 w-4 text-destructive" />
                  )}
                  <span
                    className={item.passed ? "text-text-primary" : "text-destructive font-medium"}
                  >
                    {item.label}
                  </span>
                </li>
              ))}
            </ul>
          </div>

          {/* Step-up PIN */}
          <div className="space-y-1.5">
            <Label htmlFor="step-up-pin">Code de confirmation (step-up)</Label>
            <Input
              id="step-up-pin"
              type="password"
              autoComplete="off"
              placeholder="Saisir votre code PIN"
              value={stepUpPin}
              onChange={(e) => setStepUpPin(e.target.value)}
              disabled={saving}
            />
          </div>

          {/* Notes */}
          <div className="space-y-1.5">
            <Label htmlFor="conversion-notes">Notes de conversion (optionnel)</Label>
            <Textarea
              id="conversion-notes"
              placeholder="Commentaires pour la conversion…"
              rows={2}
              value={conversionNotes}
              onChange={(e) => setConversionNotes(e.target.value)}
              disabled={saving}
            />
          </div>

          {/* Error */}
          {error && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/50 bg-destructive/10 p-3 text-sm text-destructive">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span>{error}</span>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={onClose} disabled={saving}>
            Annuler
          </Button>
          <Button onClick={handleConvert} disabled={!canConvert}>
            {saving ? (
              <>
                <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                Conversion…
              </>
            ) : (
              "Convertir en OT"
            )}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
