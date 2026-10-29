// ADR-003: the mobile shell's own IPC wrapper. `get_mobile_device_security` exists only for the
// Android field app (src-tauri/src/commands/mobile.rs); no desktop service wraps it.

import { z } from "zod";

import { invokeSilent } from "@/lib/ipc-invoke";

const MobileDeviceSecuritySchema = z.object({
  platform: z.enum(["android", "desktop"]),
  screen_lock_required: z.boolean(),
  screen_lock_present: z.boolean().nullable(),
});

export type MobileDeviceSecurity = z.infer<typeof MobileDeviceSecuritySchema>;

/** Whether the phone must have a screen lock and whether it has one. Needs no session. */
export async function getMobileDeviceSecurity(): Promise<MobileDeviceSecurity> {
  const raw = await invokeSilent<unknown>("get_mobile_device_security");
  return MobileDeviceSecuritySchema.parse(raw);
}

/**
 * The phone is refused when a screen lock is required and not confirmed present. An unknown
 * state (`null`) under a required lock is refused too: nothing is stored on an unverified phone.
 */
export function isScreenLockRefused(security: MobileDeviceSecurity): boolean {
  return security.screen_lock_required && security.screen_lock_present !== true;
}
