// DESIGN PREVIEW ONLY: a stand-in for the Tauri IPC bridge answering the field app's engine
// commands with sample data, so the REAL screens (not copies) render in a plain browser. Each
// scenario changes a few answers. Never imported by the field app.

import {
  SAMPLE_ASSETS,
  SAMPLE_REFERENCE,
  SAMPLE_SYNC,
  SAMPLE_TECHNICIAN,
  SAMPLE_URGENCY,
  SAMPLE_WORK_ORDERS,
} from "./sample-data";

export type Answer = (args: Record<string, unknown> | undefined) => unknown;
export type Answers = Record<string, Answer>;

/** A promise that never settles: the screen stays on its "in progress" state. */
const pending: Answer = () => new Promise(() => undefined);

const TENANT = "tnt-demo";
const DOMAIN_IDS = Object.keys(SAMPLE_REFERENCE);

function session(authenticated: boolean) {
  return {
    is_authenticated: authenticated,
    is_locked: false,
    user_id: authenticated ? SAMPLE_TECHNICIAN.id : null,
    username: authenticated ? SAMPLE_TECHNICIAN.username : null,
    display_name: authenticated ? SAMPLE_TECHNICIAN.display_name : null,
    is_admin: false,
    force_password_change: false,
    expires_at: null,
    last_activity_at: null,
  };
}

function onboarding(complete: boolean) {
  return {
    complete,
    status: complete ? "active" : "uninitialized",
    pending_online_validation: false,
    tenant_id: complete ? TENANT : null,
  };
}

/** The answers of a phone already activated, joined and signed in. */
export function signedInAnswers(): Answers {
  return {
    get_mobile_device_security: () => ({
      platform: "android",
      screen_lock_required: true,
      screen_lock_present: true,
    }),
    get_last_startup_event: () => ({ stage: "ready" }),
    health_check: () => ({ status: "ok", version: "0.9.2", db_connected: true, locale: "fr" }),
    get_app_info: () => ({
      version: "0.9.2",
      build_mode: "release",
      os: "android",
      arch: "aarch64",
      app_name: "Maintafox",
      default_locale: "fr",
    }),
    get_product_license_onboarding_state: () => onboarding(true),
    get_activation_bootstrap_state: () => ({
      tenant_id: TENANT,
      company_display_name: "Usine de démonstration — Blida",
      has_tenant_admin: true,
      plant_join: "sign_in",
    }),
    get_session_info: () => session(true),
    login: () => ({ session_info: session(true) }),
    logout: () => null,
    list_wo: () => ({
      items: SAMPLE_WORK_ORDERS,
      total: SAMPLE_WORK_ORDERS.length,
      hidden_outside_scope: 0,
      hidden_without_site: 0,
    }),
    list_reference_domains: () =>
      DOMAIN_IDS.map((code, i) => ({
        id: i + 1,
        code,
        name: code,
        structure_type: "flat",
        governance_level: "plant",
        governance_category: "B",
        audience: "company",
        is_extendable: true,
        validation_rules_json: null,
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
      })),
    list_reference_sets: (args) => [
      {
        id: Number(args?.["domainId"]) * 10,
        domain_id: args?.["domainId"],
        version_no: 1,
        status: "published",
        effective_from: null,
        created_by_id: null,
        created_at: "2026-01-01T00:00:00Z",
        published_at: "2026-01-01T00:00:00Z",
      },
    ],
    list_reference_values: (args) => {
      const setId = Number(args?.["setId"]);
      const code = DOMAIN_IDS[setId / 10 - 1] ?? "";
      return (SAMPLE_REFERENCE[code] ?? []).map(([value, label], i) => ({
        id: setId * 10 + i,
        set_id: setId,
        parent_id: null,
        code: value,
        label,
        description: null,
        sort_order: i,
        color_hex: null,
        icon_name: null,
        semantic_tag: null,
        external_code: null,
        is_active: true,
        metadata_json: null,
      }));
    },
    list_urgency_levels: () => SAMPLE_URGENCY,
    search_assets: () => SAMPLE_ASSETS,
    get_sync_worker_status: () => SAMPLE_SYNC,
    run_sync_now: () => null,
    "plugin:event|listen": () => 1,
    "plugin:event|unlisten": () => null,
  };
}

/** Every scenario of the preview: which engine answers differ from a signed-in phone. */
export const SCENARIO_ANSWERS: Record<string, Answers> = {
  "security-refused": {
    get_mobile_device_security: () => ({
      platform: "android",
      screen_lock_required: true,
      screen_lock_present: false,
    }),
  },
  startup: {
    get_last_startup_event: () => ({ stage: "migrations_complete", applied: 0 }),
    health_check: pending,
  },
  "activation-key": {
    get_product_license_onboarding_state: () => onboarding(false),
  },
  "activation-unreachable": {
    get_product_license_onboarding_state: () => onboarding(false),
    // The very failure the owner saw on the phone, now shown in plain words.
    preview_product_activation: () => ({
      kind: "network_error",
      message:
        "Control API request interrupted: error sending request for url (http://127.0.0.1:47831/api/v1/activation/preview)",
    }),
  },
  "activation-confirm": {
    get_product_license_onboarding_state: () => onboarding(false),
    preview_product_activation: () => ({
      kind: "answered",
      answer: {
        tenant_id: TENANT,
        license_id: "lic-demo",
        machine_fingerprint: "phone",
        tenant_display_name: "Usine de démonstration — Blida",
        edition: "Entreprise",
        is_initialized: true,
      },
    }),
  },
  "plant-joining": {
    get_activation_bootstrap_state: () => ({
      tenant_id: TENANT,
      has_tenant_admin: true,
      plant_join: "joining",
    }),
    request_plant_join: pending,
  },
  signin: {
    get_session_info: () => session(false),
  },
  "signin-refused": {
    get_session_info: () => session(false),
    login: () => {
      throw { code: "AUTH_ERROR", message: "Invalid credentials" };
    },
  },
  "wo-empty": {
    list_wo: () => ({ items: [], total: 0, hidden_outside_scope: 0, hidden_without_site: 0 }),
  },
  "sync-offline": {
    get_sync_worker_status: () => ({
      ...SAMPLE_SYNC,
      state: "offline",
      pending_outbox_count: 3,
      last_error: {
        code: "offline",
        kind: "retryable",
        message: "Cannot reach the control API: dns error",
        http_status: null,
        at: new Date().toISOString(),
      },
    }),
  },
};

/** Installs the stand-in bridge on `window` (what `@tauri-apps/api` calls). */
export function installFakeEngine(answers: Answers): void {
  let nextCallback = 1;
  const callbacks = new Map<number, unknown>();
  const internals = {
    invoke: async (cmd: string, args?: Record<string, unknown>) => {
      const answer = answers[cmd];
      if (!answer) {
        console.warn(`[preview] no sample answer for ${cmd}`);
        throw { code: "INTERNAL_ERROR", message: `preview has no answer for ${cmd}` };
      }
      return answer(args);
    },
    transformCallback: (callback: unknown) => {
      const id = nextCallback++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback: (id: number) => callbacks.delete(id),
    convertFileSrc: (path: string) => path,
    metadata: {
      currentWindow: { label: "main" },
      currentWebview: { windowLabel: "main", label: "main" },
    },
  };
  // Read by the Tauri library first (vite.mobile-preview.config.ts), so this works in a plain
  // browser and inside the app WebView, where Tauri's own bridge is read-only.
  Object.assign(window, {
    __MFX_PREVIEW_INTERNALS__: internals,
    __MFX_PREVIEW_EVENT_INTERNALS__: { unregisterListener: () => undefined },
  });
}
