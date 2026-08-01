import { createContext, useContext } from "react";

export type ProductLicenseGateRefresh = (opts?: { includeDiagnostics?: boolean }) => Promise<void>;

export type ProductLicenseGateReconcile = () => Promise<void>;

export type ProductLicenseGateContextValue = {
  refreshProductLicense: ProductLicenseGateRefresh;
  /** One-shot policy reconcile (e.g. after successful login). */
  reconcileProductLicense: ProductLicenseGateReconcile;
  /** When true, incomplete activation may show /login instead of the gate form. */
  preferLoginView: boolean;
  setPreferLoginView: (value: boolean) => void;
};

export const ProductLicenseGateContext = createContext<ProductLicenseGateContextValue | null>(null);

export function useProductLicenseGateRefresh(): ProductLicenseGateRefresh | null {
  return useContext(ProductLicenseGateContext)?.refreshProductLicense ?? null;
}

export function useProductLicenseGateReconcile(): ProductLicenseGateReconcile | null {
  return useContext(ProductLicenseGateContext)?.reconcileProductLicense ?? null;
}

export function useProductLicenseGate(): ProductLicenseGateContextValue | null {
  return useContext(ProductLicenseGateContext);
}
