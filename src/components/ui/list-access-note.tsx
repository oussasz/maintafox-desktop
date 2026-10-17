/**
 * Shown in place of a picker whose list the user may not read: names the missing right instead of
 * an empty list that would read "there is nothing to choose".
 */
import { Lock } from "lucide-react";
import { useTranslation } from "react-i18next";

import type { ReadableList } from "@/lib/list-access";
import { cn } from "@/lib/utils";

export function ListAccessNote({ list, className }: { list: ReadableList; className?: string }) {
  const { t } = useTranslation("common");
  return (
    <p
      role="note"
      data-testid={`list-access-${list}`}
      className={cn("flex items-start gap-1.5 text-xs text-text-muted", className)}
    >
      <Lock className="mt-0.5 h-3.5 w-3.5 shrink-0" aria-hidden />
      <span>{t(`listAccess.${list}`)}</span>
    </p>
  );
}
