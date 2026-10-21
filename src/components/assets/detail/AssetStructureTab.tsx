/**
 * Structure tab: the asset's place in the equipment hierarchy (parent and components), each one
 * opening its own record, and a shortcut to register a new component under this asset.
 */

import { ChevronRight, GitFork, Plus } from "lucide-react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";

import { PermissionGate } from "@/components/PermissionGate";
import { AssetStatusBadge } from "@/components/assets/AssetStatusBadge";
import { useLoad } from "@/components/assets/detail/use-load";
import { DetailSectionCard } from "@/components/detail";
import { Button } from "@/components/ui/button";
import { ErrorState, LoadingState } from "@/components/ui/data-state";
import { getAssetById, listAssetChildren, listAssetParents } from "@/services/asset-service";
import { useAssetStore } from "@/stores/asset-store";
import type { Asset, AssetHierarchyRow } from "@shared/ipc-types";
import { P } from "@shared/rbac/permissions.generated";

interface StructureData {
  parent: Asset | null;
  parentRelation: string | null;
  children: { row: AssetHierarchyRow; asset: Asset }[];
}

async function loadStructure(assetId: number): Promise<StructureData> {
  const [parents, children] = await Promise.all([
    listAssetParents(assetId),
    listAssetChildren(assetId),
  ]);
  const active = parents.filter((r) => r.effective_to == null);
  const parentRow = active.find((r) => r.relation_type === "PARENT_CHILD") ?? active[0] ?? null;
  const parent = parentRow ? await getAssetById(parentRow.parent_asset_id) : null;
  const childAssets = await Promise.all(
    children
      .filter((r) => r.effective_to == null)
      .map(async (row) => ({ row, asset: await getAssetById(row.child_asset_id) })),
  );
  return {
    parent,
    parentRelation: parentRow?.relation_type ?? null,
    children: childAssets.sort((a, b) => a.asset.asset_code.localeCompare(b.asset.asset_code)),
  };
}

interface AssetStructureTabProps {
  asset: Asset;
  onOpenAsset: (assetId: number) => void;
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

export function AssetStructureTab({ asset, onOpenAsset }: AssetStructureTabProps) {
  const { t } = useTranslation("equipment");
  const openCreateForm = useAssetStore((s) => s.openCreateForm);
  const load = useMemo(() => () => loadStructure(asset.id), [asset.id]);
  const { data, error, loading, reload } = useLoad(load, `${asset.id}:${asset.row_version}`);

  if (loading && !data) return <LoadingState />;
  if (error || !data) {
    return <ErrorState error={error} onRetry={() => void reload()} retrying={loading} />;
  }
  const parent = data.parent;

  return (
    <div className="space-y-4">
      <DetailSectionCard title={t("structure.parent")} icon={GitFork}>
        {parent ? (
          <AssetLinkRow
            asset={parent}
            relation={data.parentRelation}
            onOpen={() => onOpenAsset(parent.id)}
          />
        ) : (
          <p className="text-sm text-text-muted">{t("structure.noParent")}</p>
        )}
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
    </div>
  );
}
