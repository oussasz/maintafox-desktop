import { createContext, useContext } from "react";

export type ProductLicenseGateRefresh = (opts?: { includeDiagnostics?: boolean }) => Promise<void>;

export type ProductLicenseGateContextValue = {
  refreshProductLicense: ProductLicenseGateRefresh;
};

export const ProductLicenseGateContext = createContext<ProductLicenseGateContextValue | null>(null);

export function useProductLicenseGateRefresh(): ProductLicenseGateRefresh | null {
  return useContext(ProductLicenseGateContext)?.refreshProductLicense ?? null;
}
