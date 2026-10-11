import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  MediaFileStateBadge,
  isMediaFileAvailable,
  useMediaFileStates,
} from "@/components/sync/MediaFileStatus";
import type { MediaFileState } from "@shared/ipc-types";

const syncMocks = vi.hoisted(() => ({
  getMediaFileStates: vi.fn((_paths: string[]) => Promise.resolve([] as MediaFileState[])),
  retryMediaDownload: vi.fn((_path: string) => Promise.resolve(1)),
}));

vi.mock("@/services/sync-service", () => ({
  getMediaFileStates: (paths: string[]) => syncMocks.getMediaFileStates(paths),
  retryMediaDownload: (path: string) => syncMocks.retryMediaDownload(path),
}));

vi.mock("@/stores/sync-orchestrator-store", () => ({
  useSyncOrchestratorStore: (select: (s: { worker: null }) => unknown) => select({ worker: null }),
}));

function state(partial: Partial<MediaFileState>): MediaFileState {
  return {
    relativePath: "wo_attachments/7/a-report.pdf",
    state: "available",
    lastError: null,
    attemptCount: 0,
    ...partial,
  };
}

function Probe({ paths }: { paths: string[] }) {
  const { states } = useMediaFileStates(paths);
  return (
    <ul>
      {paths.map((p) => (
        <li key={p} data-testid={`state-${p}`}>
          {states.get(p)?.state ?? "unknown"}
        </li>
      ))}
    </ul>
  );
}

describe("MediaFileStateBadge", () => {
  beforeEach(() => {
    syncMocks.retryMediaDownload.mockClear();
  });

  it("shows nothing for an available file", () => {
    const { container } = render(<MediaFileStateBadge state={state({})} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows a file still coming from another device", () => {
    render(<MediaFileStateBadge state={state({ state: "pending" })} />);
    expect(screen.getByTestId("media-file-pending")).toBeInTheDocument();
  });

  it("shows a missing file", () => {
    render(<MediaFileStateBadge state={state({ state: "missing" })} />);
    expect(screen.getByTestId("media-file-missing")).toBeInTheDocument();
  });

  it("offers a retry for a failed download and reports it", async () => {
    const onRetried = vi.fn();
    render(
      <MediaFileStateBadge
        state={state({ state: "failed", lastError: "content hash mismatch" })}
        onRetried={onRetried}
      />,
    );
    expect(screen.getByTestId("media-file-failed")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button"));
    await waitFor(() => expect(onRetried).toHaveBeenCalledTimes(1));
    expect(syncMocks.retryMediaDownload).toHaveBeenCalledWith("wo_attachments/7/a-report.pdf");
  });

  it("treats only available (or unknown) files as openable", () => {
    expect(isMediaFileAvailable(undefined)).toBe(true);
    expect(isMediaFileAvailable(state({}))).toBe(true);
    expect(isMediaFileAvailable(state({ state: "pending" }))).toBe(false);
    expect(isMediaFileAvailable(state({ state: "failed" }))).toBe(false);
  });
});

describe("useMediaFileStates", () => {
  it("asks for the listed paths and maps the answer by path", async () => {
    syncMocks.getMediaFileStates.mockResolvedValueOnce([
      state({ relativePath: "asset_photos/1/p.jpg", state: "pending" }),
    ]);
    render(<Probe paths={["asset_photos/1/p.jpg"]} />);
    await waitFor(() =>
      expect(screen.getByTestId("state-asset_photos/1/p.jpg")).toHaveTextContent("pending"),
    );
    expect(syncMocks.getMediaFileStates).toHaveBeenCalledWith(["asset_photos/1/p.jpg"]);
  });
});
