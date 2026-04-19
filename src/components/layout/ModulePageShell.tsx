import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { mfLayout } from "@/design-system/tokens";
import { cn } from "@/lib/utils";

export interface ModulePageShellProps {
  /** If set, replaces the default icon + title + description block */
  headerLeft?: ReactNode;
  icon?: LucideIcon;
  title?: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  /** Second row under header (search/filters) — uses `mfLayout.moduleFilterBar` */
  filterBar?: React.ReactNode;
  children: ReactNode;
  className?: string;
  /**
   * Classes for the padded region inside the workspace scroll area.
   * Defaults to `mfLayout.moduleWorkspaceBody` (`p-4`, same as DI list gutter).
   * Pass `null` for full-bleed content (e.g. kanban).
   */
  bodyClassName?: string | null;
}

/**
 * Standard module layout matching **Demandes d’intervention** and **Ordres de travail**:
 * `moduleRoot` → header row → optional filter row → workspace → scrollable inner.
 */
export function ModulePageShell({
  headerLeft,
  icon: Icon,
  title,
  description,
  actions,
  filterBar,
  children,
  className,
  bodyClassName,
}: ModulePageShellProps) {
  const left =
    headerLeft ??
    (title != null ? (
      <div className={cn(mfLayout.moduleTitleRow, "items-start sm:items-center")}>
        {Icon ? <Icon className={mfLayout.moduleHeaderIcon} /> : null}
        <div className="min-w-0">
          <h1 className={mfLayout.moduleTitle}>{title}</h1>
          {description ? <p className="mt-0.5 text-sm text-text-muted">{description}</p> : null}
        </div>
      </div>
    ) : null);

  const inner =
    bodyClassName === null ? (
      children
    ) : (
      <div className={bodyClassName ?? mfLayout.moduleWorkspaceBody}>{children}</div>
    );

  return (
    <div className={cn(mfLayout.moduleRoot, className)}>
      <header className={mfLayout.moduleHeader}>
        <div className="min-w-0 flex-1">{left}</div>
        {actions ? <div className={mfLayout.moduleHeaderActions}>{actions}</div> : null}
      </header>
      {filterBar}
      <div className={mfLayout.moduleWorkspace}>
        <div className={mfLayout.moduleWorkspaceInner}>{inner}</div>
      </div>
    </div>
  );
}
