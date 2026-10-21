/**
 * Saving a generated report to disk from the Analytics pages: native Save dialog, honest
 * feedback ("Saved: file.pdf" + open the folder), cancellation is not an error.
 */
import { CheckCircle2, FolderOpen, X } from "lucide-react";
import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { mfTone } from "@/design-system/tone";
import { revealInFolder } from "@/export/download";
import { isExportCancelled } from "@/export/types";
import { describeAppError } from "@/lib/app-error";
import { cn } from "@/lib/utils";
import { saveExportedDocument } from "@/services/reports-service";
import { pushAppToast } from "@/store/app-toast-store";
import type { ExportedBinaryDocument } from "@shared/ipc-types";

export function fileNameOf(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

function reportFailure(err: unknown) {
  const { title, description } = describeAppError(err);
  pushAppToast({ title, ...(description ? { description } : {}), variant: "destructive" });
}

/**
 * `save(load)` fetches the document (IPC failures are toasted by `invoke`), opens the Save
 * dialog and remembers the written path for {@link SavedReportNotice}.
 */
export function useReportFileSave() {
  const { t } = useTranslation("reports");
  const [savedPath, setSavedPath] = useState<string | null>(null);

  const save = useCallback(
    async (load: () => Promise<ExportedBinaryDocument>): Promise<boolean> => {
      let doc: ExportedBinaryDocument;
      try {
        doc = await load();
      } catch {
        // `invoke` already showed the translated failure in the global toast.
        return false;
      }
      try {
        const path = await saveExportedDocument(doc, t("save.dialogTitle"));
        setSavedPath(path);
        return true;
      } catch (err) {
        if (!isExportCancelled(err)) reportFailure(err);
        return false;
      }
    },
    [t],
  );

  return { save, savedPath, clearSaved: () => setSavedPath(null) };
}

export function SavedReportNotice({ path, onDismiss }: { path: string; onDismiss: () => void }) {
  const { t } = useTranslation("reports");
  return (
    <div
      role="status"
      className={cn(
        "flex flex-wrap items-center gap-2 rounded-md border px-3 py-2 text-sm",
        mfTone.success.panel,
        mfTone.success.text,
      )}
    >
      <CheckCircle2 className="h-4 w-4 shrink-0" aria-hidden />
      <span className="min-w-0 flex-1 truncate">{t("save.saved", { file: fileNameOf(path) })}</span>
      <Button
        type="button"
        size="sm"
        variant="outline"
        className="h-9"
        onClick={() => void revealInFolder(path).catch(reportFailure)}
      >
        <FolderOpen className="mr-1.5 h-4 w-4" aria-hidden />
        {t("save.openFolder")}
      </Button>
      <Button
        type="button"
        size="icon"
        variant="ghost"
        className="h-9 w-9"
        aria-label={t("save.dismiss")}
        onClick={onDismiss}
      >
        <X className="h-4 w-4" aria-hidden />
      </Button>
    </div>
  );
}
