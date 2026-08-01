/** Badge / dot colors for `personnel.availability_status` (PRD §6.6). */

export function personnelAvailabilityBadgeClass(status: string): string {
  switch (status) {
    case "available":
      return "border border-emerald-500/30 bg-emerald-500/15 text-emerald-800 dark:text-emerald-200";
    case "assigned":
      return "border border-blue-500/30 bg-blue-500/15 text-blue-800 dark:text-blue-200";
    case "in_training":
      return "border border-amber-500/30 bg-amber-500/15 text-amber-900 dark:text-amber-100";
    case "on_leave":
      return "border border-gray-400/40 bg-gray-500/10 text-gray-800 dark:text-gray-200";
    case "blocked":
      return "border border-red-500/30 bg-red-500/15 text-red-800 dark:text-red-200";
    case "inactive":
      return "border border-slate-500/30 bg-slate-500/15 text-slate-800 dark:text-slate-200";
    default:
      return "border border-border bg-muted text-muted-foreground";
  }
}

export function personnelAvailabilityDotClass(status: string): string {
  switch (status) {
    case "available":
      return "bg-emerald-500";
    case "assigned":
      return "bg-blue-500";
    case "in_training":
      return "bg-amber-500";
    case "on_leave":
      return "bg-gray-400";
    case "blocked":
      return "bg-red-500";
    case "inactive":
      return "bg-slate-500";
    default:
      return "bg-muted-foreground";
  }
}
