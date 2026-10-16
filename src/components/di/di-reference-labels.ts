/**
 * Resolve DI Category B reference labels for display surfaces.
 * Catalog is SSOT — never invent labels; fall back to code/id when missing.
 */

import { useCallback, useEffect, useState } from "react";

import {
  getReferenceValue,
  listPublishedReferenceValuesByDomainCode,
} from "@/services/reference-service";

const DOMAIN_ORIGIN = "DI.ORIGIN";
/** Failure symptoms shared by requests, the OT close-out and RAMS (ISO 14224). */
const DOMAIN_SYMPTOM = "WORK.SYMPTOMS";
const DOMAIN_REQUEST_TYPE = "DI.REQUEST_TYPE";

/** code (upper) → label */
export async function loadDiOriginLabelMap(): Promise<Map<string, string>> {
  const values = await listPublishedReferenceValuesByDomainCode(DOMAIN_ORIGIN);
  const map = new Map<string, string>();
  for (const v of values) {
    map.set(v.code.trim().toUpperCase(), v.label);
  }
  return map;
}

export async function loadDiRequestTypeLabelMap(): Promise<Map<string, string>> {
  const values = await listPublishedReferenceValuesByDomainCode(DOMAIN_REQUEST_TYPE);
  const map = new Map<string, string>();
  for (const v of values) {
    map.set(v.code.trim().toUpperCase(), v.label);
  }
  return map;
}

/** id → label */
export async function loadDiSymptomLabelMap(): Promise<Map<number, string>> {
  const values = await listPublishedReferenceValuesByDomainCode(DOMAIN_SYMPTOM);
  const map = new Map<number, string>();
  for (const v of values) {
    map.set(v.id, v.label);
  }
  return map;
}

export function resolveDiOriginLabel(
  map: Map<string, string>,
  originType: string | null | undefined,
): string {
  const code = (originType ?? "").trim();
  if (!code) return "";
  return map.get(code.toUpperCase()) ?? code;
}

export function resolveDiRequestTypeLabel(
  map: Map<string, string>,
  requestType: string | null | undefined,
): string {
  const code = (requestType ?? "").trim();
  if (!code) return "";
  return map.get(code.toUpperCase()) ?? code;
}

export function resolveDiSymptomLabel(
  map: Map<number, string>,
  symptomId: number | null | undefined,
): string {
  if (symptomId == null) return "";
  return map.get(symptomId) ?? String(symptomId);
}

/** One-shot id lookup when a full map is not loaded (detail/print). */
export async function fetchDiSymptomLabel(symptomId: number | null | undefined): Promise<string> {
  if (symptomId == null) return "";
  try {
    const v = await getReferenceValue(symptomId);
    return v.label || String(symptomId);
  } catch {
    return String(symptomId);
  }
}

/**
 * Hook: loads origin (+ optional symptom) label maps once for list/detail UIs.
 */
export function useDiReferenceLabels(options?: { includeSymptoms?: boolean }) {
  const includeSymptoms = options?.includeSymptoms ?? false;
  const [originMap, setOriginMap] = useState<Map<string, string>>(() => new Map());
  const [requestTypeMap, setRequestTypeMap] = useState<Map<string, string>>(() => new Map());
  const [symptomMap, setSymptomMap] = useState<Map<number, string>>(() => new Map());

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const [origins, requestTypes] = await Promise.all([
          loadDiOriginLabelMap(),
          loadDiRequestTypeLabelMap(),
        ]);
        if (!cancelled) {
          setOriginMap(origins);
          setRequestTypeMap(requestTypes);
        }
        if (includeSymptoms) {
          const symptoms = await loadDiSymptomLabelMap();
          if (!cancelled) setSymptomMap(symptoms);
        }
      } catch {
        // Keep empty maps — display falls back to raw code/id.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [includeSymptoms]);

  const originLabel = useCallback(
    (code: string | null | undefined) => resolveDiOriginLabel(originMap, code),
    [originMap],
  );

  const requestTypeLabel = useCallback(
    (code: string | null | undefined) => resolveDiRequestTypeLabel(requestTypeMap, code),
    [requestTypeMap],
  );

  const symptomLabel = useCallback(
    (id: number | null | undefined) => resolveDiSymptomLabel(symptomMap, id),
    [symptomMap],
  );

  return { originLabel, requestTypeLabel, symptomLabel, originMap, requestTypeMap, symptomMap };
}

/** Reasons of the review decisions (COMPANY lists in Données de référence). */
export const DI_RETURN_REASON_DOMAIN = "DI.RETURN_REASON";
export const DI_DEFERRAL_REASON_DOMAIN = "DI.DEFERRAL_REASON";

async function loadCodeLabelMap(domainCode: string): Promise<Map<string, string>> {
  const values = await listPublishedReferenceValuesByDomainCode(domainCode);
  const map = new Map<string, string>();
  for (const v of values) map.set(v.code.trim().toUpperCase(), v.label);
  return map;
}

/**
 * Hook: labels of the return-for-information and deferral reasons, read from the plant's lists.
 * Falls back to the stored code when a reason was retired from the list.
 */
export function useDiDecisionReasonLabels() {
  const [returnMap, setReturnMap] = useState<Map<string, string>>(() => new Map());
  const [deferralMap, setDeferralMap] = useState<Map<string, string>>(() => new Map());

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      loadCodeLabelMap(DI_RETURN_REASON_DOMAIN),
      loadCodeLabelMap(DI_DEFERRAL_REASON_DOMAIN),
    ])
      .then(([ret, def]) => {
        if (cancelled) return;
        setReturnMap(ret);
        setDeferralMap(def);
      })
      .catch(() => {
        // Keep empty maps — display falls back to the stored code.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const returnReasonLabel = useCallback(
    (code: string | null | undefined) => resolveDiOriginLabel(returnMap, code),
    [returnMap],
  );
  const deferralReasonLabel = useCallback(
    (code: string | null | undefined) => resolveDiOriginLabel(deferralMap, code),
    [deferralMap],
  );
  return { returnReasonLabel, deferralReasonLabel };
}
