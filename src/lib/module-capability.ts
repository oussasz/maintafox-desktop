/**
 * Maps RBAC permission prefixes to commercial module capability keys.
 * Soft phase: missing/empty capability maps do not hide modules.
 */

/** `labelKey`: the module's menu name (`shell:nav.*`), for "not in your licence" messages. */
const PERMISSION_PREFIX_TO_MODULE: Array<{ prefix: string; module: string; labelKey?: string }> = [
  { prefix: "eq.", module: "equipment", labelKey: "nav.equipment" },
  { prefix: "di.", module: "requests", labelKey: "nav.requests" },
  { prefix: "ot.", module: "work_orders", labelKey: "nav.workOrders" },
  { prefix: "inv.", module: "inventory", labelKey: "nav.inventory" },
  { prefix: "per.", module: "personnel", labelKey: "nav.personnel" },
  { prefix: "trn.", module: "training", labelKey: "nav.training" },
  { prefix: "org.", module: "organization", labelKey: "nav.org" },
  { prefix: "ref.", module: "reference", labelKey: "nav.lookups" },
  { prefix: "plan.", module: "planning", labelKey: "nav.planning" },
  { prefix: "pm.", module: "pm", labelKey: "nav.pm" },
  { prefix: "ptw.", module: "permits", labelKey: "nav.permits" },
  { prefix: "ins.", module: "inspections", labelKey: "nav.inspections" },
  { prefix: "rep.", module: "reports", labelKey: "nav.analytics" },
  { prefix: "ram.", module: "rams", labelKey: "nav.reliability" },
  { prefix: "fin.", module: "finance", labelKey: "nav.budget" },
  { prefix: "sync.", module: "sync" },
  { prefix: "erp.", module: "erp" },
];

export function moduleCapabilityFromPermission(permission: string): string | null {
  for (const entry of PERMISSION_PREFIX_TO_MODULE) {
    if (permission.startsWith(entry.prefix)) {
      return entry.module;
    }
  }
  return null;
}

/** The menu label key (`shell` namespace) of a module capability key; null when it has none. */
export function moduleLabelKey(module: string): string | null {
  return PERMISSION_PREFIX_TO_MODULE.find((e) => e.module === module)?.labelKey ?? null;
}

export function parseCapabilityMap(
  capabilityMapJson: string | null | undefined,
): Record<string, boolean> {
  if (!capabilityMapJson || capabilityMapJson.trim() === "" || capabilityMapJson.trim() === "{}") {
    return {};
  }
  try {
    const parsed = JSON.parse(capabilityMapJson) as Record<string, unknown>;
    const out: Record<string, boolean> = {};
    for (const [key, value] of Object.entries(parsed)) {
      if (typeof value === "boolean") {
        out[key] = value;
      }
    }
    return out;
  } catch {
    return {};
  }
}

/**
 * Soft enforcement: when the map is empty (no envelope / legacy), allow all.
 * When a module key is present and false, block. Missing key while map non-empty → allow (soft).
 */
export function isModuleCapabilityAllowed(
  capabilityMap: Record<string, boolean>,
  permission: string | undefined,
): boolean {
  if (!permission) return true;
  const moduleKey = moduleCapabilityFromPermission(permission);
  if (!moduleKey) return true;
  if (Object.keys(capabilityMap).length === 0) return true;
  if (!(moduleKey in capabilityMap)) return true;
  return capabilityMap[moduleKey] === true;
}
