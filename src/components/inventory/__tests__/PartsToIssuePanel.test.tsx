/**
 * Parts "à sortir" (L3-21): the storekeeper sees the parts recorded without the stock right,
 * issues one (a corrected quantity needs a reason) or cancels it with a reason.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { PartsToIssuePanel } from "@/components/inventory/PartsToIssuePanel";
import enCommon from "@/i18n/en/common.json";
import enInventory from "@/i18n/locale-data/en/inventory.json";
import type { WoPartToIssue } from "@shared/ipc-types";

const exec = vi.hoisted(() => ({
  listWoPartsToIssue: vi.fn(),
  issueWoPartToIssue: vi.fn(),
  cancelWoPartToIssue: vi.fn(),
}));
const inv = vi.hoisted(() => ({ listInventoryStockBalances: vi.fn() }));
vi.mock("@/services/wo-execution-service", () => exec);
vi.mock("@/services/inventory-service", () => inv);
vi.mock("@/store/app-toast-store", () => ({ pushAppToast: vi.fn() }));

function row(partial: Partial<WoPartToIssue> = {}): WoPartToIssue {
  return {
    id: 7,
    work_order_id: 3,
    article_id: 4,
    article_ref: null,
    article_label: "BRG-6207",
    quantity_planned: 3,
    quantity_used: 3,
    unit_cost: null,
    stock_location_id: 11,
    reservation_id: null,
    quantity_reserved: 0,
    quantity_issued: 0,
    notes: null,
    origin: "execution_added",
    consumption_status: "used",
    is_required: true,
    unit_symbol: "pc",
    pending_issue_qty: 3,
    wo_code: "OT-0042",
    wo_title: "Pump bearing",
    wo_status_code: "in_progress",
    location_label: "MAIN/BIN-1",
    ...partial,
  };
}

beforeAll(async () => {
  await i18next.use(initReactI18next).init({
    lng: "en",
    resources: { en: { common: enCommon, inventory: enInventory } },
    interpolation: { escapeValue: false },
  });
});

beforeEach(() => {
  vi.clearAllMocks();
  exec.listWoPartsToIssue.mockResolvedValue([row()]);
  exec.issueWoPartToIssue.mockResolvedValue({ id: 7, work_order_id: 3 });
  exec.cancelWoPartToIssue.mockResolvedValue({ id: 7, work_order_id: 3 });
  inv.listInventoryStockBalances.mockResolvedValue([]);
});

describe("PartsToIssuePanel", () => {
  it("lists the parts to issue with their work order and quantity", async () => {
    render(<PartsToIssuePanel />);
    expect(await screen.findByText("OT-0042")).toBeInTheDocument();
    expect(screen.getByText("BRG-6207")).toBeInTheDocument();
    expect(screen.getByText("MAIN/BIN-1")).toBeInTheDocument();
    expect(screen.getByText("Parts to issue (1)")).toBeInTheDocument();
  });

  it("says when nothing is to issue", async () => {
    exec.listWoPartsToIssue.mockResolvedValue([]);
    render(<PartsToIssuePanel />);
    expect(await screen.findByText("No part to issue.")).toBeInTheDocument();
  });

  it("issues the requested quantity as it is", async () => {
    const onChanged = vi.fn();
    render(<PartsToIssuePanel onChanged={onChanged} />);
    fireEvent.click(await screen.findByRole("button", { name: "Issue" }));
    fireEvent.click(screen.getByRole("button", { name: "Issue from stock" }));
    await waitFor(() =>
      expect(exec.issueWoPartToIssue).toHaveBeenCalledWith({
        wo_part_id: 7,
        quantity: 3,
        stock_location_id: null,
        correction_reason: null,
      }),
    );
    expect(onChanged).toHaveBeenCalled();
  });

  it("asks for a reason when the issued quantity is corrected", async () => {
    render(<PartsToIssuePanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Issue" }));
    fireEvent.change(screen.getByLabelText("Issued quantity"), { target: { value: "2" } });
    const confirm = screen.getByRole("button", { name: "Issue from stock" });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason for the correction"), {
      target: { value: "only two in the bin" },
    });
    expect(confirm).toBeEnabled();
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(exec.issueWoPartToIssue).toHaveBeenCalledWith(
        expect.objectContaining({ quantity: 2, correction_reason: "only two in the bin" }),
      ),
    );
  });

  it("cancels only with a reason, and moves no stock", async () => {
    render(<PartsToIssuePanel />);
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    const confirm = screen.getByRole("button", { name: "Cancel the part" });
    expect(confirm).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Reason"), {
      target: { value: "taken from the last job" },
    });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(exec.cancelWoPartToIssue).toHaveBeenCalledWith({
        wo_part_id: 7,
        reason: "taken from the last job",
      }),
    );
    expect(exec.issueWoPartToIssue).not.toHaveBeenCalled();
  });

  it("asks where the part leaves from when the line has no location", async () => {
    exec.listWoPartsToIssue.mockResolvedValue([
      row({ stock_location_id: null, location_label: null }),
    ]);
    inv.listInventoryStockBalances.mockResolvedValue([
      {
        id: 1,
        article_id: 4,
        article_code: "BRG-6207",
        article_name: "Bearing",
        warehouse_id: 1,
        warehouse_code: "MAIN",
        warehouse_name: null,
        location_id: 11,
        location_code: "BIN-1",
        location_name: null,
        on_hand_qty: 5,
        reserved_qty: 0,
        available_qty: 5,
        updated_at: "2026-11-04T00:00:00Z",
      },
    ]);
    render(<PartsToIssuePanel />);
    expect(await screen.findByText("To choose")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Issue" }));
    await waitFor(() =>
      expect(inv.listInventoryStockBalances).toHaveBeenCalledWith({
        article_id: 4,
        warehouse_id: null,
      }),
    );
    expect(screen.getByRole("button", { name: "Issue from stock" })).toBeDisabled();
  });
});
