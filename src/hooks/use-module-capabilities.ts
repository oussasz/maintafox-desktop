import { useEffect, useState } from "react";

import { capabilityMapForEdition, parseEditionId } from "@/lib/edition-capability-catalog";
import { parseCapabilityMap } from "@/lib/module-capability";
import { ENTITLEMENTS_UPDATED_EVENT, getEntitlementSummary } from "@/services/entitlement-service";
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
        if (Object.keys(parsed).length === 0) {
          const onboarding = await getProductLicenseOnboardingState().catch(() => null);
          const fallbackEdition =
            parseEditionId(onboarding?.license_edition) ?? parseEditionId(summary.tier);
          if (fallbackEdition) {
            parsed = capabilityMapForEdition(fallbackEdition);
          }
        }
        setCapabilityMap(parsed);
      } catch {
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
