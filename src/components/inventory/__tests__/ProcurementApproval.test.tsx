/**
 * Purchase approval visibility: who approves a submitted document, and why the signed-in user
 * cannot (no approval right, or they created it).
 */
import { render, renderHook, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ApproverRolesLine,
  useApprovalBlocker,
  useApprovalBlockerFor,
} from "@/components/inventory/ProcurementApproval";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type * as InventoryService from "@/services/inventory-service";
import { P } from "@shared/rbac/permissions.generated";

type InventoryServiceModule = typeof InventoryService;

const svc = vi.hoisted(() => ({ listProcurementApproverRoles: vi.fn() }));
vi.mock("@/services/inventory-service", async (importOriginal) => {
  const actual = await importOriginal<InventoryServiceModule>();
  return { ...actual, ...svc };
});

let held: string[] = [];
vi.mock("@/hooks/use-permissions", () => ({
  usePermissions: () => ({
    can: (p: string) => held.includes(p),
    canAny: (...ps: string[]) => ps.some((p) => held.includes(p)),
    isLoading: false,
  }),
}));

let sessionUserId: number | null = 7;
vi.mock("@/store/session-store", () => ({
  useSessionStore: <T,>(select: (s: { info: { user_id: number | null } }) => T) =>
    select({ info: { user_id: sessionUserId } }),
}));

describe("purchase approval visibility", () => {
  beforeAll(async () => {
    if (!i18next.isInitialized) {
      await i18next.use(initReactI18next).init({
        lng: "en",
        fallbackLng: "en",
        ns: ["inventory"],
        defaultNS: "inventory",
        resources: { en: { inventory: enInventory } },
        interpolation: { escapeValue: false },
      });
    } else {
      i18next.addResourceBundle("en", "inventory", enInventory, true, true);
    }
  });

  beforeEach(() => {
    svc.listProcurementApproverRoles.mockReset();
    held = [P.INV_VIEW, P.INV_PROCURE, P.INV_APPROVE];
    sessionUserId = 7;
  });

  it("greys out approval for a buyer without the approval right", () => {
    held = [P.INV_VIEW, P.INV_PROCURE];
    const { result } = renderHook(() => useApprovalBlocker(3));
    expect(result.current).toBe('You do not have the "Approve purchasing" right.');
  });

  it("greys out approval for the person who created the document", () => {
    const { result } = renderHook(() => useApprovalBlocker(7));
    expect(result.current).toBe("You created this document: another person must approve it.");
  });

  it("greys out approval without the purchasing right the commands need first", () => {
    held = [P.INV_VIEW, P.INV_APPROVE];
    const { result } = renderHook(() => useApprovalBlocker(3));
    expect(result.current).toMatch(/"Manage purchasing" right/);
  });

  it("row menus: rejecting one's own document needs only the rights", () => {
    const { result } = renderHook(() => useApprovalBlockerFor());
    expect(result.current(7)).toMatch(/You created this document/);
    expect(result.current(7, false)).toBeNull();
    held = [P.INV_VIEW, P.INV_PROCURE];
    expect(renderHook(() => useApprovalBlockerFor()).result.current(3, false)).toMatch(
      /"Approve purchasing" right/,
    );
  });

  it("lets another approver try (limits are checked by the backend)", () => {
    const { result } = renderHook(() => useApprovalBlocker(3));
    expect(result.current).toBeNull();
  });

  it("names the roles that may approve the document", async () => {
    svc.listProcurementApproverRoles.mockResolvedValue({
      document_type: "PURCHASE_ORDER",
      document_id: 12,
      amount: 85000,
      amount_unknown_reason: null,
      role_names: ["Administrator", "Maintenance Supervisor"],
    });
    render(<ApproverRolesLine documentType="PURCHASE_ORDER" documentId={12} />);
    expect(
      await screen.findByText("To be approved by: Administrator, Maintenance Supervisor"),
    ).toBeInTheDocument();
    expect(svc.listProcurementApproverRoles).toHaveBeenCalledWith("PURCHASE_ORDER", 12);
  });

  it("says when no role may approve and when the amount is unknown", async () => {
    svc.listProcurementApproverRoles.mockResolvedValue({
      document_type: "REQUISITION",
      document_id: 4,
      amount: null,
      amount_unknown_reason: "line A has no price",
      role_names: [],
    });
    render(<ApproverRolesLine documentType="REQUISITION" documentId={4} />);
    expect(await screen.findByText(/No role may approve this amount/)).toBeInTheDocument();
    expect(screen.getByText(/The amount is not known/)).toBeInTheDocument();
  });
});
