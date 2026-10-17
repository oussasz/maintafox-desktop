import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ArchiveExplorer } from "@/components/archive/ArchiveExplorer";
import { PermissionProvider } from "@/contexts/PermissionContext";
import type {
  ArchiveFilterInput,
  ArchiveItemDetail,
  ArchiveItemSummary,
} from "@/services/archive-service";
import { P } from "@shared/rbac/permissions.generated";

const mockGetMyPermissions = vi.fn();

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "name" in opts ? `${key}:${String(opts["name"])}` : key,
    i18n: { language: "en" },
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(vi.fn())),
}));

vi.mock("@/services/rbac-service", () => ({
  getMyPermissions: (...args: unknown[]) => mockGetMyPermissions(...args),
}));

// PermissionContext only loads permissions for an authenticated session.
vi.mock("@/services/auth-service", () => ({
  getSessionInfo: vi.fn().mockResolvedValue({
    is_authenticated: true,
    is_locked: false,
    user_id: 1,
    username: "admin",
    display_name: "Admin",
    is_admin: true,
    force_password_change: false,
    expires_at: null,
    last_activity_at: null,
    password_expires_in_days: null,
    pin_configured: false,
    tenant_id: "t1",
    token_tenant_id: "t1",
  }),
}));

// Step-up is exercised by its own tests; here the protected action runs directly.
vi.mock("@/hooks/use-step-up", () => ({
  useStepUp: () => ({
    withStepUp: <T,>(action: () => Promise<T>) => action(),
    StepUpDialogElement: <></>,
  }),
}));

const io = vi.hoisted(() => ({
  saveBlobWithDialog: vi.fn((_blob: Blob, name: string) => Promise.resolve(`C:/out/${name}`)),
  pushAppToast: vi.fn(),
}));
vi.mock("@/export/download", () => ({ saveBlobWithDialog: io.saveBlobWithDialog }));
vi.mock("@/store/app-toast-store", () => ({ pushAppToast: io.pushAppToast }));

const archiveMocks = vi.hoisted(() => {
  const base = {
    source_state: "closed",
    archive_reason_code: "lifecycle",
    archived_by_id: 1,
    retention_policy_id: 3,
    restore_policy: "admin_only",
    restore_until_at: null,
    legal_hold: false,
    checksum_sha256: null,
  };
  const fixtureItems: ArchiveItemSummary[] = [
    {
      ...base,
      id: 1,
      source_module: "wo",
      source_record_id: "WO-1001",
      archive_class: "operational_history",
      archived_at: "2015-06-01T12:00:00.000Z",
      search_text: "alpha note",
    },
    {
      ...base,
      id: 2,
      source_module: "wo",
      source_record_id: "WO-1002",
      archive_class: "operational_history",
      archived_at: "2025-01-15T12:00:00.000Z",
      search_text: "beta note",
    },
    {
      ...base,
      id: 3,
      source_module: "di",
      source_record_id: "42",
      archive_class: "soft_delete",
      archived_at: "2024-03-01T12:00:00.000Z",
      search_text: "gamma",
    },
  ];

  return {
    fixtureItems,
    listArchiveItems: vi.fn((_filter: ArchiveFilterInput) => Promise.resolve(fixtureItems)),
    getArchiveItem: vi.fn((_id: number): Promise<ArchiveItemDetail> => Promise.reject(new Error())),
    exportArchiveItems: vi.fn((_payload: unknown) =>
      Promise.resolve({ items: [{ archive_item_id: 1, payload_json: { code: "WO-1001" } }] }),
    ),
    purgeArchiveItems: vi.fn((_payload: unknown) =>
      Promise.resolve({ strict_mode: true, purged_item_ids: [1], blocked_items: [] }),
    ),
    restoreArchiveItem: vi.fn((_payload: unknown) =>
      Promise.resolve({ archive_item_id: 1, restore_action_id: 1, message: "ok" }),
    ),
    setLegalHold: vi.fn((_payload: unknown) => Promise.resolve()),
  };
});

vi.mock("@/services/archive-service", () => ({
  listArchiveItems: (filter: ArchiveFilterInput) => archiveMocks.listArchiveItems(filter),
  getArchiveItem: (id: number) => archiveMocks.getArchiveItem(id),
  exportArchiveItems: (payload: unknown) => archiveMocks.exportArchiveItems(payload),
  purgeArchiveItems: (payload: unknown) => archiveMocks.purgeArchiveItems(payload),
  restoreArchiveItem: (payload: unknown) => archiveMocks.restoreArchiveItem(payload),
  setLegalHold: (payload: unknown) => archiveMocks.setLegalHold(payload),
}));

function renderExplorer() {
  return render(
    <PermissionProvider>
      <ArchiveExplorer />
    </PermissionProvider>,
  );
}

function lastListArchiveFilter(): ArchiveFilterInput | undefined {
  const calls = archiveMocks.listArchiveItems.mock.calls as unknown as [ArchiveFilterInput][];
  return calls[calls.length - 1]?.[0];
}

function permission(name: string) {
  return {
    name,
    description: "",
    category: "archive",
    is_dangerous: false,
    requires_step_up: false,
  };
}

function detailOf(item: ArchiveItemSummary, payload: unknown): ArchiveItemDetail {
  return {
    item,
    payload: {
      id: 10,
      archive_item_id: item.id,
      payload_json: payload,
      workflow_history_json: null,
      attachment_manifest_json: null,
      config_version_refs_json: null,
      payload_size_bytes: 12,
    },
    actions: [],
    retention_policy: {
      id: 3,
      module_code: "wo",
      archive_class: "operational_history",
      retention_years: 7,
      purge_mode: "manual_approval",
      allow_restore: true,
      allow_purge: true,
      requires_legal_hold_check: true,
    },
    checksum_valid: true,
  };
}

function readBlob(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(reader.error);
    reader.readAsText(blob);
  });
}

const firstItem = () => archiveMocks.fixtureItems[0] as ArchiveItemSummary;

async function openFirstRecord() {
  renderExplorer();
  fireEvent.click(await screen.findByText("WO-1001"));
  await screen.findByText("detail.checksumOk");
}

describe("ArchiveExplorer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetMyPermissions.mockResolvedValue([
      permission(P.ARC_EXPORT),
      permission(P.ARC_RESTORE),
      permission(P.ARC_PURGE),
    ]);
    archiveMocks.listArchiveItems.mockImplementation(() =>
      Promise.resolve(archiveMocks.fixtureItems),
    );
    archiveMocks.getArchiveItem.mockResolvedValue(
      detailOf(firstItem(), { code: "WO-1001", title: "Pump P-101 seal" }),
    );
  });

  it("filters through the translated module / type / year tree", async () => {
    renderExplorer();
    await screen.findByText("WO-1001");

    const tree = screen.getByTestId("archive-folder-tree");
    fireEvent.click(within(tree).getByText("module.wo"));
    await waitFor(() => expect(lastListArchiveFilter()?.source_module).toBe("wo"));

    fireEvent.change(screen.getByPlaceholderText("explorer.searchPlaceholder"), {
      target: { value: "alpha" },
    });
    await waitFor(() => expect(lastListArchiveFilter()?.search_text).toBe("alpha"));

    fireEvent.click(screen.getByRole("checkbox", { name: "explorer.legalHoldOnly" }));
    await waitFor(() => expect(lastListArchiveFilter()?.legal_hold).toBe(true));

    fireEvent.click(within(tree).getByText("module.di"));
    await waitFor(() => expect(lastListArchiveFilter()?.source_module).toBe("di"));

    const years = within(tree).getAllByRole("button", { name: /^2024/ });
    fireEvent.click(years[0] as HTMLElement);
    await waitFor(() => {
      expect(lastListArchiveFilter()?.date_from).toBe("2024-01-01");
      expect(lastListArchiveFilter()?.date_to).toContain("2024-12-31");
    });
  });

  it("names records by business code and never shows a bare database id", async () => {
    renderExplorer();
    expect(await screen.findByText("WO-1001")).toBeInTheDocument();
    expect(screen.queryByText("42")).not.toBeInTheDocument();
    expect(screen.getByText("record.unnamed")).toBeInTheDocument();
    expect(screen.queryByText("operational_history")).not.toBeInTheDocument();
  });

  it("exports through the native save dialog and reports a failure in a toast", async () => {
    await openFirstRecord();
    expect(screen.getByText("WO-1001 — Pump P-101 seal")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "detail.export" }));
    await waitFor(() => expect(io.saveBlobWithDialog).toHaveBeenCalledTimes(1));
    const [blob, fileName] = io.saveBlobWithDialog.mock.calls[0] as [Blob, string];
    expect(fileName).toBe("archive-WO-1001-Pump-P-101-seal.json");
    expect(JSON.parse(await readBlob(blob))).toEqual({ code: "WO-1001" });

    io.saveBlobWithDialog.mockRejectedValueOnce(new Error("disk full"));
    fireEvent.click(screen.getByRole("button", { name: "detail.export" }));
    await waitFor(() =>
      expect(io.pushAppToast).toHaveBeenCalledWith(
        expect.objectContaining({ variant: "destructive", title: "disk full" }),
      ),
    );
  });

  it("asks for a reason and confirmation before a permanent delete", async () => {
    await openFirstRecord();
    fireEvent.click(screen.getByRole("button", { name: "detail.purge" }));

    const dialog = await screen.findByRole("dialog");
    const confirm = within(dialog).getByRole("button", { name: "detail.purgeConfirm" });
    expect(confirm).toBeDisabled();
    expect(archiveMocks.purgeArchiveItems).not.toHaveBeenCalled();

    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: "Retention over" } });
    fireEvent.click(confirm);
    await waitFor(() =>
      expect(archiveMocks.purgeArchiveItems).toHaveBeenCalledWith({
        archive_item_ids: [1],
        purge_reason: "Retention over",
      }),
    );
  });

  it("blocks the permanent delete while the retention period runs", async () => {
    archiveMocks.getArchiveItem.mockResolvedValue(
      detailOf({ ...firstItem(), archived_at: new Date().toISOString() }, { code: "WO-1001" }),
    );
    await openFirstRecord();
    fireEvent.click(screen.getByRole("button", { name: "detail.purge" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("detail.purgeNotEligible")).toBeInTheDocument();
    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: "x" } });
    expect(within(dialog).getByRole("button", { name: "detail.purgeConfirm" })).toBeDisabled();
  });

  it("records a restore request only after confirmation", async () => {
    await openFirstRecord();
    fireEvent.click(screen.getByRole("button", { name: "detail.restore" }));
    const dialog = await screen.findByRole("dialog");
    fireEvent.change(within(dialog).getByRole("textbox"), { target: { value: "Audit" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "detail.restoreConfirm" }));
    await waitFor(() =>
      expect(archiveMocks.restoreArchiveItem).toHaveBeenCalledWith({
        archive_item_id: 1,
        reason_note: "Audit",
      }),
    );
    expect(io.pushAppToast).toHaveBeenCalledWith(
      expect.objectContaining({ title: "detail.restoreRecorded" }),
    );
  });
});
