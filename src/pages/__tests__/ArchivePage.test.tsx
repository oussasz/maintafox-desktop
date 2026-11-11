/**
 * L7-19: the Archives page says what it holds, where closed records stay, and how to restore,
 * naming the real restore button.
 */
import { render, screen } from "@testing-library/react";
import type { ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/components/archive/ArchiveExplorer", () => ({ ArchiveExplorer: () => null }));
vi.mock("@/components/archive/RetentionPolicyPanel", () => ({ RetentionPolicyPanel: () => null }));
vi.mock("@/components/layout/ModulePageShell", () => ({
  ModulePageShell: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts?.["button"] ? `${key}[${String(opts["button"])}]` : key,
    i18n: { language: "fr" },
  }),
}));

import { ArchivePage } from "../ArchivePage";

describe("ArchivePage", () => {
  it("explains archiving and the restore path", () => {
    render(<ArchivePage />);
    expect(screen.getByRole("region", { name: "page.about.title" })).toBeInTheDocument();
    expect(screen.getByText("page.about.closed")).toBeInTheDocument();
    expect(screen.getByText("page.about.what")).toBeInTheDocument();
    expect(screen.getByText("page.about.restore[detail.restore]")).toBeInTheDocument();
  });
});
