import { PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY } from "@/services/product-license-service";

/**
 * The install's device identifier for the licence server, kept for the life of the install in the
 * same storage key as the desktop (a regenerated one would burn a device slot on each reset).
 */
export function deviceFingerprint(): string {
  try {
    const existing = localStorage.getItem(PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY);
    if (existing) return existing;
  } catch {
    // Storage unreachable: a fresh identifier is generated below and cannot be kept.
  }
  const generated = crypto.randomUUID();
  try {
    localStorage.setItem(PRODUCT_LICENSE_DEVICE_FINGERPRINT_STORAGE_KEY, generated);
  } catch {
    // Not kept: the next activation attempt generates another one.
  }
  return generated;
}

/** The name this phone gets in the plant's device list. */
export function deviceLabel(): string {
  const platform = typeof navigator !== "undefined" && navigator.platform ? navigator.platform : "";
  return platform ? `Mobile (${platform})` : "Mobile";
}
