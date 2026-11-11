/**
 * Structure tab: the asset's place in the equipment hierarchy (parent and components), each one
 * opening its own record, a shortcut to register a new component under this asset, and
 * "Déplacer le composant" (L2-03) to pose this asset on another parent or remove it to stock.
 */

import { ArrowRightLeft, ChevronRight, GitFork, Plus } from "lucide-react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetMoveComponentDialog } from "@/components/assets/AssetMoveComponentDialog";
import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { getAssetById, listAssetChildren, listAssetParents } from "@/services/asset-service";
import { useAssetStore } from "@/stores/asset-store";
import type { Asset, AssetHierarchyRow } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

/** Relation types with at most one active parent (`assets::hierarchy::SINGLE_PARENT_TYPES`). */
const SINGLE_PARENT_TYPES = ["PARENT_CHILD", "INSTALLED_IN"];

interface StructureData {
  /** Every active single-parent relation: one normally, several only from an inconsistent link. */
  parents: { row: AssetHierarchyRow; asset: Asset }[];
  children: { row: AssetHierarchyRow; asset: Asset }[];
}

async function loadStructure(assetId: number): Promise<StructureData> {
  const [parents, children] = await Promise.all([
    listAssetParents(assetId),
    listAssetChildren(assetId),
  ]);
  // The parent a component is posed on: an active single-parent relation (PARENT_CHILD or
  // INSTALLED_IN), the same rule as the server's move; other links (DRIVES, FEEDS) are not parents.
  const active = parents.filter(
    (r) => r.effective_to == null && SINGLE_PARENT_TYPES.includes(r.relation_type),
  );
  const parentAssets = await Promise.all(
    active.map(async (row) => ({ row, asset: await getAssetById(row.parent_asset_id) })),
  );
  const childAssets = await Promise.all(
    children
      .filter((r) => r.effective_to == null)
      .map(async (row) => ({ row, asset: await getAssetById(row.child_asset_id) })),
  );
  return {
    parents: parentAssets,
    children: childAssets.sort((a, b) => a.asset.asset_code.localeCompare(b.asset.asset_code)),
  };
}

interface AssetStructureTabProps {
  asset: Asset;
  onOpenAsset: (assetId: number) => void;
  /** Called after "Déplacer le composant" was recorded (the caller reloads the asset). */
  onMoved?: () => void | Promise<void>;
}

function AssetLinkRow({
  asset,
  relation,
  onOpen,
}: {
  asset: Asset;
  relation: string | null;
  onOpen: () => void;
}) {
  const { t } = useTranslation("equipment");
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex min-h-[44px] w-full items-center gap-3 rounded-md border border-surface-border px-3 py-2 text-left hover:bg-surface-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span className="font-mono text-xs text-text-muted">{asset.asset_code}</span>
      <span className="min-w-0 flex-1 truncate text-sm">{asset.asset_name}</span>
      {relation ? (
        <span className="hidden text-[11px] text-text-muted sm:inline">
          {t(`structure.relation.${relation}`, { defaultValue: t("structure.relation.other") })}
        </span>
      ) : null}
      <AssetStatusBadge code={asset.status_code} />
      <ChevronRight className="h-4 w-4 text-text-muted" aria-hidden />
    </button>
  );
}

export function AssetStructureTab({ asset, onOpenAsset, onMoved }: AssetStructureTabProps) {
  const { t } = useTranslation("equipment");
  const openCreateForm = useAssetStore((s) => s.openCreateForm);
  const [showMove, setShowMove] = useState(false);
  const load = useMemo(() => () => loadStructure(asset.id), [asset.id]);
  const { data, error, loading, reload } = useLoad(load, `${asset.id}:${asset.row_version}`);

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  // Several parents at once: the server refuses the move (GATE_ASSET_MOVE_SEVERAL_PARENTS), so it
  // is not offered; every parent is listed with the reason.
  const severalParents = data.parents.length > 1;
  const parent = data.parents.length === 1 ? (data.parents[0]?.asset ?? null) : null;

  return (
    <div className="space-y-4">
      <DetailSectionCard title={t("structure.parent")} icon={GitFork}>
        {data.parents.length === 0 ? (
          <p className="text-sm text-text-muted">{t("structure.noParent")}</p>
        ) : (
          <div className="space-y-2">
            {data.parents.map(({ row, asset: p }) => (
              <AssetLinkRow
                key={row.relation_id}
                asset={p}
                relation={row.relation_type}
                onOpen={() => onOpenAsset(p.id)}
              />
            ))}
          </div>
        )}
        {severalParents ? (
          <p role="note" className="pt-2 text-sm text-tone-warning-fg">
            {t("moveComponent.severalParents", { count: data.parents.length })}
          </p>
        ) : null}
        <PermissionGate permission={P.EQ_MANAGE}>
          <div className="flex justify-end pt-2">
            <Button
              variant="outline"
              className="h-11 gap-1.5"
              disabled={severalParents}
              onClick={() => setShowMove(true)}
            >
              <ArrowRightLeft className="h-4 w-4" aria-hidden />
              {t("moveComponent.action")}
            </Button>
          </div>
        </PermissionGate>
      </DetailSectionCard>

      <DetailSectionCard
        title={t("structure.components", { count: data.children.length })}
        icon={GitFork}
      >
        {data.children.length === 0 ? (
          <p className="text-sm text-text-muted">{t("structure.noComponents")}</p>
        ) : (
          <div className="space-y-2">
            {data.children.map(({ row, asset: child }) => (
              <AssetLinkRow
                key={row.relation_id}
                asset={child}
                relation={row.relation_type}
                onOpen={() => onOpenAsset(child.id)}
              />
            ))}
          </div>
        )}
        <PermissionGate permission={P.EQ_MANAGE}>
          <div className="flex justify-end pt-2">
            <Button
              variant="outline"
              className="h-11 gap-1.5"
              onClick={() => openCreateForm(asset)}
            >
              <Plus className="h-4 w-4" aria-hidden />
              {t("structure.addComponent")}
            </Button>
          </div>
        </PermissionGate>
      </DetailSectionCard>

      <AssetMoveComponentDialog
        open={showMove}
        asset={asset}
        currentParent={parent}
        onClose={() => setShowMove(false)}
        onMoved={async () => {
          setShowMove(false);
          await reload();
          await onMoved?.();
        }}
      />
    </div>
  );
}
