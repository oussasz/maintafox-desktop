/**
 * ReferenceModalFactory — resolves create-modal configuration from the
 * referenceType registry. Forms must not branch on domain codes with if/else.
 * Create eligibility is decided by governance capabilities, not this factory.
 */

import {
  getReferenceTypeConfig,
  modalTitle,
  type ReferenceTypeConfig,
  type ReferenceTypeId,
} from "@/components/reference/reference-types";

export interface ReferenceCreateModalConfig {
  referenceType: ReferenceTypeId;
  domainCode: string;
  title: string;
  requireParent: boolean;
  parentDomainCode?: string;
  config: ReferenceTypeConfig;
}

export function resolveReferenceCreateModal(
  referenceType: ReferenceTypeId,
  locale: string,
): ReferenceCreateModalConfig {
  const config = getReferenceTypeConfig(referenceType);
  return {
    referenceType,
    domainCode: config.domainCode,
    title: modalTitle(config, locale),
    requireParent: Boolean(config.parentDomainCode),
    ...(config.parentDomainCode ? { parentDomainCode: config.parentDomainCode } : {}),
    config,
  };
}
