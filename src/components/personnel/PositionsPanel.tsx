import { Archive, Plus, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { SmartFilterBar } from "@/components/filters/SmartFilterBar";
import type { SmartFilterDef } from "@/components/filters/smart-filter-types";
import { PositionCreateDialog } from "@/components/personnel/PositionCreateDialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useDebouncedValue } from "@/hooks/useDebouncedValue";
import { cn } from "@/lib/utils";
import { archivePosition, listPositionsFiltered } from "@/services/personnel-service";
import type { Position } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

export function PositionsPanel() {
  const { t } = useTranslation("personnel");

  const [positions, setPositions] = useState<Position[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchInput, setSearchInput] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [archiving, setArchiving] = useState<number | null>(null);

  const debouncedSearch = useDebouncedValue(searchInput, 300);

  const loadPositions = useCallback(async (search?: string) => {
    setLoading(true);
    setError(null);
    try {
      const list = await listPositionsFiltered({
        search: search?.trim() || null,
        include_inactive: true,
      });
      setPositions(list);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadPositions(debouncedSearch);
  }, [loadPositions, debouncedSearch]);

  const filterDefs = useMemo<SmartFilterDef[]>(() => [], []);

  const handleArchive = async (position: Position) => {
    setArchiving(position.id);
    try {
      await archivePosition(position.id);
      await loadPositions(debouncedSearch);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setArchiving(null);
    }
  };

  return (
    <PermissionGate permission={P.PER_POSITION_VIEW}>
      <div className="space-y-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <h2 className="text-sm font-semibold">{t("position.panel.title", "Positions")}</h2>
            <Badge variant="secondary" className="text-xs">
              {positions.length}
            </Badge>
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void loadPositions(debouncedSearch)}
              disabled={loading}
              className="gap-1.5"
            >
              <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
            </Button>
            <PermissionGate permission={P.PER_POSITION_MANAGE}>
              <Button size="sm" className="gap-1.5" onClick={() => setShowCreate(true)}>
                <Plus className="h-3.5 w-3.5" />
                {t("position.action.create", "New position")}
              </Button>
            </PermissionGate>
          </div>
        </div>

        <SmartFilterBar
          searchPlaceholder={t("position.filter.searchPlaceholder", "Search code or name…")}
          searchValue={searchInput}
          onSearchInputChange={setSearchInput}
          onSearchChange={(q) => void loadPositions(q)}
          filters={filterDefs}
          resultCount={positions.length}
          onReset={() => {
            setSearchInput("");
            void loadPositions();
          }}
        />

        {error ? <p className="text-sm text-destructive">{error}</p> : null}

        {loading && positions.length === 0 ? (
          <p className="text-sm text-text-muted">{t("common.loading")}</p>
        ) : positions.length === 0 ? (
          <p className="text-sm text-text-muted">{t("common.noData")}</p>
        ) : (
          <div className="divide-y rounded-lg border border-surface-border">
            {positions.map((pos) => (
              <div
                key={pos.id}
                className={cn(
                  "flex items-center justify-between px-4 py-2.5",
                  pos.is_active === 0 && "opacity-50",
                )}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-xs font-medium text-text-secondary">
                      {pos.code}
                    </span>
                    <span className="text-sm font-medium truncate">{pos.name}</span>
                    <Badge
                      variant="outline"
                      className="shrink-0 text-[10px] border-0 bg-surface-2 capitalize"
                    >
                      {t(`position.category.${pos.category}`, pos.category)}
                    </Badge>
                    {pos.is_active === 0 ? (
                      <Badge
                        variant="outline"
                        className="shrink-0 text-[10px] border-0 bg-amber-100 text-amber-800"
                      >
                        {t("position.status.archived", "Archived")}
                      </Badge>
                    ) : null}
                  </div>
                </div>
                <PermissionGate permission={P.PER_POSITION_MANAGE}>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="ml-2 h-7 gap-1 text-xs text-text-secondary hover:text-destructive"
                    disabled={archiving === pos.id}
                    onClick={() => void handleArchive(pos)}
                  >
                    <Archive className="h-3.5 w-3.5" />
                    {pos.is_active === 0
                      ? t("position.action.unarchive", "Restore")
                      : t("position.action.archive", "Archive")}
                  </Button>
                </PermissionGate>
              </div>
            ))}
          </div>
        )}

        <PositionCreateDialog
          open={showCreate}
          onClose={() => setShowCreate(false)}
          onCreated={() => {
            setShowCreate(false);
            void loadPositions(debouncedSearch);
          }}
        />
      </div>
    </PermissionGate>
  );
}
