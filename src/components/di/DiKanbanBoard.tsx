/**
 * DiKanbanBoard.tsx
 *
 * Kanban board view for intervention requests, inspired by the web version.
 * 5 columns: Soumises → En validation → Approuvées → En travaux → Clôturées.
 * Cards show code, title, priority badge, origin tag, and date.
 */

import { CheckCircle, ClipboardCheck, Inbox, Search, Wrench } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Card, CardContent } from "@/components/ui/card";
import type { InterventionRequest } from "@shared/ipc-types";

// ── Kanban column definitions ───────────────────────────────────────────────

interface KanbanColumnDef {
  id: string;
  label: string;
  icon: ReactNode;
  headerClass: string;
  statuses: string[];
}

const COLUMNS: KanbanColumnDef[] = [
  {
    id: "submitted",
    label: "Soumises",
    icon: <Inbox className="h-4 w-4" />,
    headerClass: "bg-blue-50 text-blue-700 border-blue-200",
    statuses: ["submitted"],
  },
  {
    id: "review",
    label: "En validation",
    icon: <Search className="h-4 w-4" />,
    headerClass: "bg-amber-50 text-amber-700 border-amber-200",
    statuses: ["pending_review", "returned_for_clarification", "screened"],
  },
  {
    id: "approved",
    label: "Approuvées",
    icon: <ClipboardCheck className="h-4 w-4" />,
    headerClass: "bg-green-50 text-green-700 border-green-200",
    statuses: ["approved_for_planning", "awaiting_approval", "deferred"],
  },
  {
    id: "work",
    label: "En travaux",
    icon: <Wrench className="h-4 w-4" />,
    headerClass: "bg-purple-50 text-purple-700 border-purple-200",
    statuses: ["converted_to_work_order"],
  },
  {
    id: "closed",
    label: "Clôturées",
    icon: <CheckCircle className="h-4 w-4" />,
    headerClass: "bg-slate-50 text-slate-600 border-slate-200",
    statuses: ["closed_as_non_executable", "archived"],
  },
];

// ── Priority styling ────────────────────────────────────────────────────────

const URGENCY_STYLE: Record<string, { class: string; icon: string }> = {
  critical: { class: "bg-red-100 text-red-700", icon: "✗" },
  high: { class: "bg-orange-100 text-orange-800", icon: "↑" },
  medium: { class: "bg-yellow-100 text-yellow-800", icon: "–" },
  low: { class: "bg-green-100 text-green-800", icon: "↓" },
};

const URGENCY_BAR: Record<string, string> = {
  critical: "bg-red-600",
  high: "bg-orange-500",
  medium: "bg-amber-400",
  low: "bg-emerald-400",
};

const ORIGIN_LABELS: Record<string, string> = {
  operator: "Opérateur",
  technician: "Technicien",
  inspection: "Inspection",
  pm: "Préventif",
  iot: "IoT",
  quality: "Qualité",
  hse: "HSE",
  production: "Production",
  external: "Externe",
};

// ── Props ───────────────────────────────────────────────────────────────────

interface DiKanbanBoardProps {
  items: InterventionRequest[];
  onCardClick: (di: InterventionRequest) => void;
}

// ── Component ───────────────────────────────────────────────────────────────

export function DiKanbanBoard({ items, onCardClick }: DiKanbanBoardProps) {
  // Group items by column
  const grouped = new Map<string, InterventionRequest[]>();
  for (const col of COLUMNS) {
    grouped.set(col.id, []);
  }
  for (const item of items) {
    const col = COLUMNS.find((c) => c.statuses.includes(item.status));
    if (col) {
      grouped.get(col.id)?.push(item);
    }
  }

  return (
    <div className="relative flex flex-col h-full">
      <div className="flex gap-3 flex-1 overflow-x-auto p-1 pb-2">
        {COLUMNS.map((col) => (
          <KanbanColumn
            key={col.id}
            def={col}
            items={grouped.get(col.id) ?? []}
            onCardClick={onCardClick}
          />
        ))}
      </div>
    </div>
  );
}

// ── Column ──────────────────────────────────────────────────────────────────

function KanbanColumn({
  def,
  items,
  onCardClick,
}: {
  def: KanbanColumnDef;
  items: InterventionRequest[];
  onCardClick: (di: InterventionRequest) => void;
}) {
  return (
    <div className="flex flex-col min-w-[220px] max-w-[260px] flex-1 rounded-lg border bg-muted/30">
      {/* Column header */}
      <div
        className={`flex items-center gap-2 px-3 py-2.5 rounded-t-lg border-b font-medium text-sm ${def.headerClass}`}
      >
        {def.icon}
        <span>{def.label}</span>
        <Badge variant="secondary" className="ml-auto text-[10px] h-5 min-w-[20px] justify-center">
          {items.length}
        </Badge>
      </div>

      {/* Cards */}
      <div className="flex-1 overflow-y-auto p-2 space-y-2">
        {items.length === 0 ? (
          <p className="text-xs text-muted-foreground text-center py-8">—</p>
        ) : (
          items.map((di) => <DiKanbanCard key={di.id} di={di} onClick={() => onCardClick(di)} />)
        )}
      </div>
    </div>
  );
}

// ── Card ────────────────────────────────────────────────────────────────────

function DiKanbanCard({ di, onClick }: { di: InterventionRequest; onClick: () => void }) {
  const { t } = useTranslation("di");
  const urgency = URGENCY_STYLE[di.reported_urgency];
  const desc = di.description.length > 80 ? `${di.description.substring(0, 80)}…` : di.description;
  const urgencyBarClass = URGENCY_BAR[di.reported_urgency] ?? "bg-gray-300";

  return (
    <Card
      className="cursor-pointer hover:shadow-md transition-shadow border overflow-hidden"
      onClick={onClick}
    >
      <div className={`h-1 w-full ${urgencyBarClass}`} />
      <CardContent className="p-3 space-y-2">
        {/* Title row */}
        <div className="flex items-start justify-between gap-2">
          <h4 className="text-xs font-semibold leading-tight line-clamp-2">
            <span className="text-muted-foreground">{di.code}</span>
            {di.is_modified && (
              <Badge className="ml-1 bg-amber-100 text-amber-800 border-0 text-[9px] px-1 py-0 align-middle">
                {t("review.modified")}
              </Badge>
            )}{" "}
            {di.title}
          </h4>
        </div>

        {/* Description snippet */}
        <p className="text-[11px] text-muted-foreground leading-snug">{desc}</p>

        {/* Badges row */}
        <div className="flex flex-wrap gap-1">
          {/* Priority badge */}
          {urgency && (
            <Badge
              variant="outline"
              className={`text-[10px] border-0 px-1.5 py-0 ${urgency.class}`}
            >
              {urgency.icon} {t(`priority.${di.reported_urgency}`)}
            </Badge>
          )}

          {/* Origin tag */}
          <Badge variant="outline" className="text-[10px] px-1.5 py-0">
            {ORIGIN_LABELS[di.origin_type] ?? di.origin_type}
          </Badge>

          {/* Safety flag */}
          {di.safety_flag && (
            <Badge variant="destructive" className="text-[10px] px-1.5 py-0">
              ⚠ Sécurité
            </Badge>
          )}

          {/* Date */}
          <Badge
            variant="outline"
            className="text-[10px] px-1.5 py-0 ml-auto text-muted-foreground"
          >
            {formatShortDate(di.submitted_at)}
          </Badge>
        </div>
      </CardContent>
    </Card>
  );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function formatShortDate(iso: string): string {
  try {
    return new Date(iso).toLocaleDateString("fr-FR", {
      day: "2-digit",
      month: "2-digit",
    });
  } catch {
    return iso;
  }
}
