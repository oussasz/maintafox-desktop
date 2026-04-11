/**
 * WoKanbanBoard.tsx
 *
 * Kanban board view for work orders, same design as DiKanbanBoard.
 * 5 columns: Brouillon → Planifié → En cours → Clôture → Terminés.
 * Cards show code, title, type badge, urgency badge, responsible, and date.
 */

import { CheckCircle, ClipboardCheck, FileEdit, Play, Settings } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import { formatShortDate } from "@/utils/format-date";
import type { WorkOrder } from "@shared/ipc-types";

// ── Kanban column definitions ───────────────────────────────────────────────

interface KanbanColumnDef {
  id: string;
  i18nKey: string;
  icon: ReactNode;
  headerClass: string;
  statuses: string[];
}

const COLUMNS: KanbanColumnDef[] = [
  {
    id: "draft",
    i18nKey: "kanban.draft",
    icon: <FileEdit className="h-4 w-4" />,
    headerClass: "bg-gray-50 text-gray-700 border-gray-200",
    statuses: ["draft", "awaiting_approval"],
  },
  {
    id: "planned",
    i18nKey: "kanban.planned",
    icon: <Settings className="h-4 w-4" />,
    headerClass: "bg-blue-50 text-blue-700 border-blue-200",
    statuses: ["planned", "ready_to_schedule", "assigned", "waiting_for_prerequisite"],
  },
  {
    id: "executing",
    i18nKey: "kanban.executing",
    icon: <Play className="h-4 w-4" />,
    headerClass: "bg-amber-50 text-amber-700 border-amber-200",
    statuses: ["in_progress", "paused", "on_hold"],
  },
  {
    id: "closing",
    i18nKey: "kanban.closing",
    icon: <ClipboardCheck className="h-4 w-4" />,
    headerClass: "bg-purple-50 text-purple-700 border-purple-200",
    statuses: ["mechanically_complete", "technically_verified"],
  },
  {
    id: "done",
    i18nKey: "kanban.done",
    icon: <CheckCircle className="h-4 w-4" />,
    headerClass: "bg-slate-50 text-slate-600 border-slate-200",
    statuses: ["closed", "cancelled"],
  },
];

// ── Urgency styling ─────────────────────────────────────────────────────────

const URGENCY_STYLE: Record<number, { class: string; icon: string }> = {
  1: { class: "bg-red-100 text-red-700", icon: "✗" },
  2: { class: "bg-orange-100 text-orange-800", icon: "↑" },
  3: { class: "bg-yellow-100 text-yellow-800", icon: "–" },
  4: { class: "bg-green-100 text-green-800", icon: "↓" },
};

// ── Props ───────────────────────────────────────────────────────────────────

interface WoKanbanBoardProps {
  items: WorkOrder[];
  onCardClick: (wo: WorkOrder) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function WoKanbanBoard({ items, onCardClick }: WoKanbanBoardProps) {
  const { t, i18n } = useTranslation("ot");

  // Group items by column
  const grouped = new Map<string, WorkOrder[]>();
  for (const col of COLUMNS) {
    grouped.set(col.id, []);
  }
  for (const item of items) {
    const sc = item.status_code ?? "draft";
    const col = COLUMNS.find((c) => c.statuses.includes(sc));
    if (col) {
      grouped.get(col.id)?.push(item);
    }
  }

  return (
    <div className="flex gap-3 h-full overflow-x-auto p-4">
      {COLUMNS.map((col) => (
        <KanbanColumn
          key={col.id}
          def={col}
          items={grouped.get(col.id) ?? []}
          onCardClick={onCardClick}
          t={t}
          locale={i18n.language}
        />
      ))}
    </div>
  );
}

// ── Column ──────────────────────────────────────────────────────────────────

function KanbanColumn({
  def,
  items,
  onCardClick,
  t,
  locale,
}: {
  def: KanbanColumnDef;
  items: WorkOrder[];
  onCardClick: (wo: WorkOrder) => void;
  t: (key: string) => string;
  locale: string;
}) {
  return (
    <div className="flex flex-col min-w-[260px] w-[260px] shrink-0 rounded-lg border bg-muted/30">
      {/* Column header */}
      <div
        className={`flex items-center gap-2 px-3 py-2.5 rounded-t-lg border-b font-medium text-sm ${def.headerClass}`}
      >
        {def.icon}
        <span>{t(def.i18nKey)}</span>
        <Badge variant="secondary" className="ml-auto text-[10px] h-5 min-w-[20px] justify-center">
          {items.length}
        </Badge>
      </div>

      {/* Cards */}
      <div className="flex-1 overflow-y-auto p-2 space-y-2">
        {items.length === 0 ? (
          <p className="text-xs text-muted-foreground text-center py-8">—</p>
        ) : (
          items.map((wo) => (
            <WoKanbanCard key={wo.id} wo={wo} onClick={() => onCardClick(wo)} locale={locale} />
          ))
        )}
      </div>
    </div>
  );
}

// ── Card ────────────────────────────────────────────────────────────────────

function WoKanbanCard({
  wo,
  onClick,
  locale,
}: {
  wo: WorkOrder;
  onClick: () => void;
  locale: string;
}) {
  const urgency = wo.urgency_level != null ? URGENCY_STYLE[wo.urgency_level] : null;
  const desc =
    wo.description && wo.description.length > 80
      ? `${wo.description.substring(0, 80)}…`
      : wo.description;

  return (
    <Card
      className="cursor-pointer hover:shadow-md transition-shadow border"
      onClick={onClick}
      tabIndex={0}
      role="button"
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
    >
      <CardContent className="p-3 space-y-2">
        {/* Title row */}
        <div className="flex items-start justify-between gap-2">
          <h4 className="text-xs font-semibold leading-tight line-clamp-2">
            <span className="text-muted-foreground">{wo.code}</span> {wo.title}
          </h4>
        </div>

        {/* Description snippet */}
        {desc && <p className="text-[11px] text-muted-foreground leading-snug">{desc}</p>}

        {/* Badges row */}
        <div className="flex flex-wrap gap-1">
          {/* Urgency badge */}
          {urgency && (
            <Badge
              variant="outline"
              className={`text-[10px] border-0 px-1.5 py-0 ${urgency.class}`}
            >
              {urgency.icon} {wo.urgency_label ?? ""}
            </Badge>
          )}

          {/* Type tag */}
          {wo.type_label && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0">
              {wo.type_label}
            </Badge>
          )}

          {/* Equipment */}
          {wo.asset_label && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-muted-foreground">
              {wo.asset_label}
            </Badge>
          )}

          {/* Responsible */}
          {wo.responsible_username && (
            <Badge variant="outline" className="text-[10px] px-1.5 py-0 text-muted-foreground">
              {wo.responsible_username}
            </Badge>
          )}

          {/* Date */}
          {wo.planned_start && (
            <Badge
              variant="outline"
              className="text-[10px] px-1.5 py-0 ml-auto text-muted-foreground"
            >
              {formatShortDate(wo.planned_start, locale)}
            </Badge>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
