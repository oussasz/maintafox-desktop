import type { LibraryDocumentCategory } from "@shared/ipc-types";

/** URL segment → DB category */
export const DOC_CATEGORY_SLUGS = [
  "technical-manuals",
  "sops",
  "safety-protocols",
  "compliance-certificates",
] as const;

export type DocumentationCategorySlug = (typeof DOC_CATEGORY_SLUGS)[number];

export const SLUG_TO_CATEGORY: Record<DocumentationCategorySlug, LibraryDocumentCategory> = {
  "technical-manuals": "technical_manuals",
  sops: "sops",
  "safety-protocols": "safety_protocols",
  "compliance-certificates": "compliance_certificates",
};

export const CATEGORY_TO_SLUG: Record<LibraryDocumentCategory, DocumentationCategorySlug> = {
  technical_manuals: "technical-manuals",
  sops: "sops",
  safety_protocols: "safety-protocols",
  compliance_certificates: "compliance-certificates",
};

export function isDocumentationCategorySlug(s: string | undefined): s is DocumentationCategorySlug {
  return s != null && (DOC_CATEGORY_SLUGS as readonly string[]).includes(s);
}
