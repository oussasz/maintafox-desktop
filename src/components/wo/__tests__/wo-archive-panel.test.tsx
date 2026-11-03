import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import enOt from "@/i18n/locale-data/en/ot.json";
import type { WoListFilter, WorkOrder } from "@shared/ipc-types";

import { WoArchivePanel } from "../WoArchivePanel";

function lookup(key: string): string | undefined {
  const value = key
    .split(".")
    .reduce<unknown>((node, part) => (node as Record<string, unknown> | undefined)?.[part], enOt);
  return typeof value === "string" ? value : undefined;
}

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      (lookup(key) ?? key).replace(/\{\{(\w+)\}\}/g, (_, k: string) => String(opts?.[k] ?? "")),
    i18n: { language: "en" },
  }),
}));

const woSvc = vi.hoisted(() => ({ listWos: vi.fn() }));
vi.mock("@/services/wo-service", () => woSvc);

function wo(id: number): WorkOrder {
  return { id, code: `OT-${id}`, title: `Closed ${id}`, status_code: "closed" } as WorkOrder;
}

describe("WoArchivePanel", () => {
  beforeEach(() => {
    woSvc.listWos.mockReset();
    woSvc.listWos.mockImplementation(({ offset, search }: WoListFilter) =>
      Promise.resolve({
        items: search ? [wo(99)] : offset ? [wo(2)] : [wo(1)],
        total: search ? 1 : 549,
        hidden_outside_scope: 0,
        hidden_without_site: 0,
      }),
    );
  });

  it("shows the real archive count before the panel is opened", async () => {
    render(<WoArchivePanel />);
    expect(await screen.findByText("549")).toBeInTheDocument();
    expect(woSvc.listWos).toHaveBeenCalledWith(
      expect.objectContaining({ status_codes: ["closed", "cancelled"], offset: 0 }),
    );
  });

  it("searches on the server and loads more rows", async () => {
    render(<WoArchivePanel />);
    fireEvent.click(await screen.findByRole("button", { name: /Archive/ }));
    fireEvent.click(await screen.findByRole("button", { name: /Load more \(548 left\)/ }));
    await waitFor(() =>
      expect(woSvc.listWos).toHaveBeenLastCalledWith(expect.objectContaining({ offset: 1 })),
    );
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "huile" } });
    await waitFor(() =>
      expect(woSvc.listWos).toHaveBeenLastCalledWith(
        expect.objectContaining({ search: "huile", offset: 0 }),
      ),
    );
  });
});
