import { useEffect, useState } from "react";

import { capabilityMapForEdition, parseEditionId } from "@/lib/edition-capability-catalog";
import { parseCapabilityMap } from "@/lib/module-capability";
import {
  ENTITLEMENTS_UPDATED_EVENT,
  getEntitlementSummary,
} from "@/services/entitlement-service";
import { getProductLicenseOnboardingState } from "@/services/product-license-service";

/**
 * Soft-phase module capability map from the active entitlement envelope.
 * Empty map ⇒ fall back to commercial edition on the activation claim,
 * otherwise show all modules (true legacy / unknown edition).
 */
export function useModuleCapabilities(): {
  capabilityMap: Record<string, boolean>;
  loading: boolean;
} {
  const [capabilityMap, setCapabilityMap] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(true);
  const [reloadToken, setReloadToken] = useState(0);

  useEffect(() => {
    const onUpdated = () => setReloadToken((n) => n + 1);
    window.addEventListener(ENTITLEMENTS_UPDATED_EVENT, onUpdated);
    return () => window.removeEventListener(ENTITLEMENTS_UPDATED_EVENT, onUpdated);
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const summary = await getEntitlementSummary();
        if (cancelled) return;
        let parsed = parseCapabilityMap(summary.capability_map_json);
        let source: "envelope" | "edition-fallback" | "empty-legacy" = "envelope";
        let fallbackEdition: string | null = null;
        if (Object.keys(parsed).length === 0) {
          const onboarding = await getProductLicenseOnboardingState().catch(() => null);
          fallbackEdition =
            parseEditionId(onboarding?.license_edition) ??
            parseEditionId(summary.tier);
          if (fallbackEdition) {
            parsed = capabilityMapForEdition(fallbackEdition);
            source = "edition-fallback";
          } else {
            source = "empty-legacy";
          }
        }
        // #region agent log
        fetch("http://127.0.0.1:7917/ingest/ad1591f6-dd5d-401c-a069-f101c6318963", {
          method: "POST",
          headers: { "Content-Type": "application/json", "X-Debug-Session-Id": "a6aab3" },
          body: JSON.stringify({
            sessionId: "a6aab3",
            runId: "post-fix",
            hypothesisId: "A-H-fallback",
            location: "use-module-capabilities.ts:load",
            message: "entitlement summary loaded for nav",
            data: {
              source,
              fallbackEdition,
              envelopeId: summary.envelope_id ?? null,
              tier: summary.tier ?? null,
              parsedKeyCount: Object.keys(parsed).length,
              falseKeys: Object.entries(parsed)
                .filter(([, v]) => v === false)
                .map(([k]) => k),
              trueKeys: Object.entries(parsed)
                .filter(([, v]) => v === true)
                .map(([k]) => k),
              reloadToken,
            },
            timestamp: Date.now(),
          }),
        }).catch(() => {});
        // #endregion
        setCapabilityMap(parsed);
      } catch (err) {
        // #region agent log
        fetch("http://127.0.0.1:7917/ingest/ad1591f6-dd5d-401c-a069-f101c6318963", {
          method: "POST",
          headers: { "Content-Type": "application/json", "X-Debug-Session-Id": "a6aab3" },
          body: JSON.stringify({
            sessionId: "a6aab3",
            runId: "post-fix",
            hypothesisId: "B",
            location: "use-module-capabilities.ts:catch",
            message: "getEntitlementSummary failed; soft-allow empty map",
            data: { error: err instanceof Error ? err.message : String(err) },
            timestamp: Date.now(),
          }),
        }).catch(() => {});
        // #endregion
        if (!cancelled) setCapabilityMap({});
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [reloadToken]);

  return { capabilityMap, loading };
}
