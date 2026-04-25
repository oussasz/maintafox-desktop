/**
 * NodeTypeManagerPanel.tsx — GAP ORG-01
 *
 * Dialog-based admin panel for managing org node types.
 * Uses the centered floating dialog pattern (like DI details).
 * Supports list, add, edit, and deactivate with visual icon picker,
 * color swatches, and capability toggles.
 */

import {
  Building,
  Building2,
  CircuitBoard,
  Cog,
  Factory,
  Hammer,
  HardHat,
  Layers,
  type LucideIcon,
  MapPin,
  Network,
  Pencil,
  Plus,
  Settings,
  Trash2,
  Users,
  Warehouse,
  Wrench,
  X,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  createOrgNodeType,
  deactivateOrgNodeType,
  getOrgNodeTypeUsageCount,
  listOrgNodeTypes,
  updateOrgNodeType,
} from "@/services/org-service";
import type { OrgNodeType } from "@shared/ipc-types";

// ─── Icon catalogue (visual picker for non-technical users) ──────────────────

interface IconOption {
  key: string;
  Icon: LucideIcon;
}

const ICON_OPTIONS: IconOption[] = [
  { key: "building-2", Icon: Building2 },
  { key: "building", Icon: Building },
  { key: "factory", Icon: Factory },
  { key: "warehouse", Icon: Warehouse },
  { key: "layers", Icon: Layers },
  { key: "network", Icon: Network },
  { key: "users", Icon: Users },
  { key: "map-pin", Icon: MapPin },
  { key: "wrench", Icon: Wrench },
  { key: "hammer", Icon: Hammer },
  { key: "cog", Icon: Cog },
  { key: "settings", Icon: Settings },
  { key: "hard-hat", Icon: HardHat },
  { key: "zap", Icon: Zap },
  { key: "circuit-board", Icon: CircuitBoard },
];

/** Resolve an icon key to its Lucide component — returns Building2 as fallback */
function resolveIcon(key: string | null | undefined): LucideIcon {
  if (!key) return Building2;
  return ICON_OPTIONS.find((o) => o.key === key)?.Icon ?? Building2;
}

// ─── Color preset swatches (Tailwind palette) ────────────────────────────────

const COLOR_SWATCHES = [
  "#3b82f6", // blue-500
  "#22c55e", // green-500
  "#eab308", // yellow-500
  "#a855f7", // purple-500
  "#ef4444", // red-500
  "#f97316", // orange-500
  "#06b6d4", // cyan-500
  "#ec4899", // pink-500
  "#14b8a6", // teal-500
  "#6366f1", // indigo-500
  "#64748b", // slate-500
  "#78716c", // stone-500
];

// ─── Capability flag definitions ─────────────────────────────────────────────

const CAPABILITY_FLAGS = [
  { key: "can_host_assets", labelKey: "capabilities.asset" },
  { key: "can_own_work", labelKey: "capabilities.work" },
  { key: "can_carry_cost_center", labelKey: "capabilities.cost" },
  { key: "can_aggregate_kpis", labelKey: "capabilities.kpi" },
  { key: "can_receive_permits", labelKey: "capabilities.permit" },
] as const;

type CapFlag = (typeof CAPABILITY_FLAGS)[number]["key"];

// ─── Types ───────────────────────────────────────────────────────────────────

interface NodeTypeFormState {
  label: string;
  code: string;
  icon_key: string;
  color: string;
  capabilities: Record<CapFlag, boolean>;
}

const EMPTY_FORM: NodeTypeFormState = {
  label: "",
  code: "",
  icon_key: "",
  color: COLOR_SWATCHES[0] ?? "#3b82f6",
  capabilities: {
    can_host_assets: false,
    can_own_work: false,
    can_carry_cost_center: false,
    can_aggregate_kpis: false,
    can_receive_permits: false,
  },
};

// ─── Props ───────────────────────────────────────────────────────────────────

interface NodeTypeManagerPanelProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  structureModelId: number | null;
  onTypesChanged: () => void;
}

export function NodeTypeManagerPanel({
  open,
  onOpenChange,
  structureModelId,
  onTypesChanged,
}: NodeTypeManagerPanelProps) {
  const { t } = useTranslation("org");
  const [types, setTypes] = useState<OrgNodeType[]>([]);
  const [loading, setLoading] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState<NodeTypeFormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [usageCounts, setUsageCounts] = useState<Record<number, number>>({});

  const loadTypes = useCallback(async () => {
    if (!structureModelId) return;
    setLoading(true);
    try {
      const list = await listOrgNodeTypes(structureModelId);
      setTypes(list);
      // Load usage counts in parallel
      const counts = await Promise.all(
        list.map(async (nt) => {
          const count = await getOrgNodeTypeUsageCount(nt.id);
          return [nt.id, count] as const;
        }),
      );
      setUsageCounts(Object.fromEntries(counts));
    } finally {
      setLoading(false);
    }
  }, [structureModelId]);

  useEffect(() => {
    if (open) void loadTypes();
  }, [open, loadTypes]);

  const startCreate = () => {
    setEditingId(null);
    setCreating(true);
    setForm(EMPTY_FORM);
  };

  const startEdit = (nt: OrgNodeType) => {
    setCreating(false);
    setEditingId(nt.id);
    setForm({
      label: nt.label,
      code: nt.code,
      icon_key: nt.icon_key ?? "",
      color: nt.color ?? COLOR_SWATCHES[0] ?? "#3b82f6",
      capabilities: {
        can_host_assets: nt.can_host_assets,
        can_own_work: nt.can_own_work,
        can_carry_cost_center: nt.can_carry_cost_center,
        can_aggregate_kpis: nt.can_aggregate_kpis,
        can_receive_permits: nt.can_receive_permits,
      },
    });
  };

  const cancelForm = () => {
    setEditingId(null);
    setCreating(false);
    setForm(EMPTY_FORM);
  };

  const saveForm = async () => {
    if (!structureModelId || !form.label.trim()) return;
    setSaving(true);
    try {
      if (creating) {
        await createOrgNodeType({
          structure_model_id: structureModelId,
          code: form.code.trim() || form.label.trim().toLowerCase().replace(/\s+/g, "_"),
          label: form.label.trim(),
          ...(form.icon_key ? { icon_key: form.icon_key } : {}),
          ...(form.color ? { color: form.color } : {}),
          ...form.capabilities,
          is_root_type: false,
        });
      } else if (editingId !== null) {
        await updateOrgNodeType({
          id: editingId,
          label: form.label.trim(),
          icon_key: form.icon_key || null,
          color: form.color || null,
          ...form.capabilities,
        });
      }
      cancelForm();
      await loadTypes();
      onTypesChanged();
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async (nt: OrgNodeType) => {
    const usage = usageCounts[nt.id] ?? 0;
    if (usage > 0) return; // blocked — UI should not allow
    setSaving(true);
    try {
      await deactivateOrgNodeType(nt.id);
      await loadTypes();
      onTypesChanged();
    } finally {
      setSaving(false);
    }
  };

  const setCapability = (flag: CapFlag, checked: boolean) => {
    setForm((prev) => ({
      ...prev,
      capabilities: { ...prev.capabilities, [flag]: checked },
    }));
  };

  const isFormMode = creating || editingId !== null;

  return (
    <Dialog open={open} onOpenChange={(isOpen) => !isOpen && onOpenChange(false)}>
      <DialogContent
        className="max-w-3xl max-h-[85vh] flex flex-col p-0 gap-0"
        onPointerDownOutside={(e) => e.preventDefault()}
      >
        {/* ── Header ── */}
        <DialogHeader className="px-6 py-4 border-b border-surface-border">
          <DialogTitle>{t("nodeTypes.title")}</DialogTitle>
          <DialogDescription>{t("nodeTypes.description")}</DialogDescription>
        </DialogHeader>

        {/* ── Scrollable body ── */}
        <div className="flex-1 overflow-y-auto px-6 py-4 space-y-4">
          {/* Type list */}
          {!isFormMode &&
            (loading ? (
              <p className="text-sm text-text-muted">{t("nodeTypes.loading")}</p>
            ) : types.length === 0 ? (
              <p className="text-sm text-text-muted">{t("nodeTypes.empty")}</p>
            ) : (
              <div className="space-y-2">
                {types.map((nt) => {
                  const usage = usageCounts[nt.id] ?? 0;
                  const RowIcon = resolveIcon(nt.icon_key);
                  return (
                    <div
                      key={nt.id}
                      className="flex items-center gap-3 rounded-lg border border-surface-border p-3"
                    >
                      {/* Color dot + icon */}
                      <div
                        className="flex h-8 w-8 items-center justify-center rounded-full shrink-0"
                        style={{ backgroundColor: nt.color ?? "#64748b" }}
                      >
                        <RowIcon className="h-4 w-4 text-white" />
                      </div>
                      {/* Label + capabilities */}
                      <div className="flex-1 min-w-0">
                        <div className="text-sm font-medium text-text-primary truncate">
                          {nt.label}
                        </div>
                        <div className="flex flex-wrap gap-1 mt-0.5">
                          {CAPABILITY_FLAGS.filter((f) => nt[f.key]).map((f) => (
                            <Badge key={f.key} variant="secondary" className="text-[10px]">
                              {t(f.labelKey)}
                            </Badge>
                          ))}
                        </div>
                      </div>
                      {/* Actions */}
                      <div className="flex items-center gap-1">
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-7 w-7 p-0"
                          onClick={() => startEdit(nt)}
                        >
                          <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-7 w-7 p-0"
                          disabled={usage > 0 || saving}
                          title={
                            usage > 0 ? t("nodeTypes.deleteBlocked", { count: usage }) : undefined
                          }
                          onClick={() => void handleDelete(nt)}
                        >
                          <Trash2 className="h-3.5 w-3.5 text-status-danger" />
                        </Button>
                      </div>
                    </div>
                  );
                })}
              </div>
            ))}

          {/* Edit / Create form */}
          {isFormMode && (
            <div className="space-y-4">
              <div className="flex items-center justify-between">
                <h3 className="text-sm font-semibold text-text-primary">
                  {creating ? t("nodeTypes.addType") : t("nodeTypes.editType")}
                </h3>
                <Button size="sm" variant="ghost" className="h-7 w-7 p-0" onClick={cancelForm}>
                  <X className="h-4 w-4" />
                </Button>
              </div>

              {/* Label */}
              <div className="space-y-1.5">
                <Label className="text-xs">{t("nodeTypes.labelField")}</Label>
                <Input
                  value={form.label}
                  onChange={(e) => setForm((p) => ({ ...p, label: e.target.value }))}
                  placeholder={t("nodeTypes.labelPlaceholder")}
                  autoFocus
                />
              </div>

              {/* Code (only during creation) */}
              {creating && (
                <div className="space-y-1.5">
                  <Label className="text-xs">{t("nodeTypes.codeField")}</Label>
                  <Input
                    value={form.code}
                    onChange={(e) => setForm((p) => ({ ...p, code: e.target.value }))}
                    placeholder={t("nodeTypes.codePlaceholder")}
                    className="font-mono"
                  />
                </div>
              )}

              {/* Icon picker (visual dropdown) */}
              <div className="space-y-1.5">
                <Label className="text-xs">{t("nodeTypes.iconField")}</Label>
                <Select
                  value={form.icon_key || "__none__"}
                  onValueChange={(v) =>
                    setForm((p) => ({ ...p, icon_key: v === "__none__" ? "" : v }))
                  }
                >
                  <SelectTrigger className="h-10">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="__none__">
                      <span className="text-text-muted">{t("nodeTypes.noIcon")}</span>
                    </SelectItem>
                    {ICON_OPTIONS.map(({ key, Icon }) => (
                      <SelectItem key={key} value={key}>
                        <span className="flex items-center gap-2">
                          <Icon className="h-4 w-4 shrink-0" />
                          <span>{t(`nodeTypes.icons.${key}` as never)}</span>
                        </span>
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>

              {/* Color picker */}
              <div className="space-y-1.5">
                <Label className="text-xs">{t("nodeTypes.colorField")}</Label>
                <div className="flex flex-wrap gap-2">
                  {COLOR_SWATCHES.map((c) => (
                    <button
                      key={c}
                      type="button"
                      className={`h-7 w-7 rounded-full border-2 transition-transform ${
                        form.color === c ? "border-text-primary scale-110" : "border-transparent"
                      }`}
                      style={{ backgroundColor: c }}
                      onClick={() => setForm((p) => ({ ...p, color: c }))}
                    />
                  ))}
                </div>
                <Input
                  value={form.color}
                  onChange={(e) => setForm((p) => ({ ...p, color: e.target.value }))}
                  placeholder="#hex"
                  className="mt-1 h-8 w-32 font-mono text-xs"
                />
              </div>

              {/* Capabilities */}
              <div className="space-y-1.5">
                <Label className="text-xs">{t("nodeTypes.capabilitiesField")}</Label>
                <div className="space-y-2">
                  {CAPABILITY_FLAGS.map((f) => (
                    <label
                      key={f.key}
                      htmlFor={`cap-${f.key}`}
                      className="flex items-center gap-2 cursor-pointer"
                    >
                      <Checkbox
                        id={`cap-${f.key}`}
                        checked={form.capabilities[f.key]}
                        onCheckedChange={(checked) => setCapability(f.key, checked)}
                      />
                      <span className="text-sm">{t(f.labelKey)}</span>
                    </label>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* ── Footer ── */}
        <DialogFooter className="px-6 py-3 border-t border-surface-border">
          {isFormMode ? (
            <>
              <Button variant="outline" onClick={cancelForm} disabled={saving}>
                {t("nodeTypes.cancel")}
              </Button>
              <Button onClick={() => void saveForm()} disabled={saving || !form.label.trim()}>
                {saving ? t("nodeTypes.saving") : t("nodeTypes.save")}
              </Button>
            </>
          ) : (
            <>
              <Button variant="outline" onClick={() => onOpenChange(false)}>
                {t("nodeTypes.close")}
              </Button>
              <Button className="gap-2" onClick={startCreate}>
                <Plus className="h-4 w-4" />
                {t("nodeTypes.addType")}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
