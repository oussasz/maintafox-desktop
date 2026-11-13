// Which control API (licence server) this build talks to. The engine has the address compiled in
// (src-tauri/build.rs reads VITE_ADMIN_API_BASE_URL, else shared/control-plane.json); Vite inlines
// the same variable here, so the screens can say when a build is pointed at a test server.

import controlPlane from "@shared/control-plane.json";

const productionAddress = controlPlane.defaultApiBaseUrl.trim();
const override = (import.meta.env["VITE_ADMIN_API_BASE_URL"] as string | undefined)?.trim();

/** The control API address compiled into this build. */
export const serverAddress: string = override || productionAddress;

/** This build talks to another server than the real Maintafox one (local proof server, staging). */
export const isTestServer: boolean = serverAddress !== productionAddress;
