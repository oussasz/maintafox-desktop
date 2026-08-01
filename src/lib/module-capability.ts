/**
 * Maps RBAC permission prefixes to commercial module capability keys.
 * Soft phase: missing/empty capability maps do not hide modules.
 */

const PERMISSION_PREFIX_TO_MODULE: Array<{ prefix: string; module: string }> = [
  { prefix: "eq.", module: "equipment" },
  { prefix: "di.", module: "requests" },
  { prefix: "ot.", module: "work_orders" },
  { prefix: "inv.", module: "inventory" },
  { prefix: "per.", module: "personnel" },
  { prefix: "trn.", module: "personnel" },
  { prefix: "org.", module: "organization" },
  { prefix: "ref.", module: "reference" },
  { prefix: "plan.", module: "planning" },
  { prefix: "pm.", module: "pm" },
  { prefix: "ptw.", module: "permits" },
  { prefix: "ins.", module: "inspections" },
  { prefix: "rep.", module: "reports" },
  { prefix: "ram.", module: "rams" },
  { prefix: "fin.", module: "finance" },
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

export function parseCapabilityMap(capabilityMapJson: string | null | undefined): Record<string, boolean> {
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
