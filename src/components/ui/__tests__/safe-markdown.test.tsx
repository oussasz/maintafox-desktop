import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { SafeMarkdown, parseSafeMarkdown } from "../safe-markdown";

describe("SafeMarkdown", () => {
  it("renders bold, bullets and headings without showing Markdown symbols", () => {
    const { container } = render(
      <SafeMarkdown source={"## Nouveautés\n\n- **Rapide** : recherche\n- Plus clair\n\nFin."} />,
    );
    expect(container.textContent).not.toContain("**");
    expect(container.textContent).not.toContain("##");
    expect(container.querySelectorAll("li")).toHaveLength(2);
    expect(container.querySelector("strong")?.textContent).toBe("Rapide");
    expect(screen.getByText("Nouveautés")).toBeInTheDocument();
  });

  it("never turns text into HTML", () => {
    const { container } = render(<SafeMarkdown source={'<img src=x onerror="alert(1)"> **ok**'} />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain("<img");
  });

  it("can drop the leading title line", () => {
    expect(parseSafeMarkdown("# Version 1.2\n\nTexte", true)).toEqual([
      { kind: "paragraph", text: "Texte" },
    ]);
    expect(parseSafeMarkdown("# Version 1.2\n\nTexte", false)).toHaveLength(2);
  });

  it("keeps unmatched markers as plain text", () => {
    const { container } = render(<SafeMarkdown source="2 * 3 = 6 et a_b" />);
    expect(container.textContent).toBe("2 * 3 = 6 et a_b");
  });
});
