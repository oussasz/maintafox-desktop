import { Plus } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PositionCreateDialog } from "@/components/personnel/PositionCreateDialog";
import { Button } from "@/components/ui/button";
import { usePermissions } from "@/hooks/use-permissions";
import { listPositionsFiltered } from "@/services/personnel-service";
import type { Position } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface PositionComboboxProps {
  value: number | null;
  onChange: (id: number | null) => void;
  placeholder?: string;
  disabled?: boolean;
  /** Called after a new position is created (allows parent to react) */
  onPositionCreated?: (position: Position) => void;
}

export function PositionCombobox({
  value,
  onChange,
  placeholder,
  disabled = false,
  onPositionCreated,
}: PositionComboboxProps) {
  const { t } = useTranslation("personnel");
  const { can } = usePermissions();
  const canManage = can(P.PER_POSITION_MANAGE);

  const [positions, setPositions] = useState<Position[]>([]);
  const [query, setQuery] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [loading, setLoading] = useState(false);

  const loadPositions = useCallback(async () => {
    setLoading(true);
    try {
      const list = await listPositionsFiltered({ include_inactive: false });
      setPositions(list);
    } catch {
      setPositions([]);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPositions();
  }, [loadPositions]);

  const filtered = useMemo(() => {
    const term = query.trim().toLowerCase();
    if (!term) return positions.slice(0, 150);
    return positions
      .filter((p) => `${p.code} ${p.name}`.toLowerCase().includes(term))
      .slice(0, 150);
  }, [positions, query]);

  // Ensure selected value is included even if not in the filtered list
  const listWithSelected = useMemo(() => {
    if (!value) return filtered;
    const inList = filtered.some((p) => p.id === value);
    if (inList) return filtered;
    const selected = positions.find((p) => p.id === value);
    return selected ? [selected, ...filtered].slice(0, 150) : filtered;
  }, [filtered, positions, value]);

  return (
    <div className="space-y-1.5">
      <div className="flex items-start gap-1">
        <div className="flex-1 space-y-1.5">
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={
              loading
                ? t("common.loading")
                : (placeholder ?? t("position.combobox.searchPlaceholder", "Filter positions…"))
            }
            className="h-8 w-full rounded-md border border-input bg-background px-3 text-sm shadow-sm placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            disabled={disabled}
            aria-label={t("field.position")}
          />
          <select
            className="h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-sm shadow-sm focus:outline-none focus:ring-1 focus:ring-ring"
            value={value ?? ""}
            onChange={(e) => onChange(e.target.value ? Number(e.target.value) : null)}
            disabled={disabled}
          >
            <option value="">—</option>
            {listWithSelected.map((p) => (
              <option key={p.id} value={p.id}>
                {p.code} — {p.name}
              </option>
            ))}
          </select>
        </div>
        {canManage ? (
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="mt-[calc(2rem+0.375rem)] h-9 w-9 shrink-0"
            onClick={() => setShowCreate(true)}
            disabled={disabled}
            title={t("position.create.title", "New Position")}
          >
            <Plus className="h-4 w-4" />
          </Button>
        ) : null}
      </div>

      <PositionCreateDialog
        open={showCreate}
        onClose={() => setShowCreate(false)}
        onCreated={(pos) => {
          void loadPositions();
          onChange(pos.id);
          setShowCreate(false);
          onPositionCreated?.(pos);
        }}
      />
    </div>
  );
}
