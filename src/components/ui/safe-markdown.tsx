import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

/**
 * Renders a small Markdown subset (headings, bullets, bold, italic, inline code, paragraphs) as
 * React text nodes. It never builds HTML from the text, so server-supplied notes cannot inject
 * markup. Anything outside the subset is shown as plain text.
 */

type Block =
  | { kind: "heading"; text: string }
  | { kind: "list"; items: string[] }
  | { kind: "paragraph"; text: string };

const HEADING = /^#{1,6}\s+(.*)$/;
const BULLET = /^\s*(?:[-*+]|\d+[.)])\s+(.*)$/;

export function parseSafeMarkdown(source: string, skipLeadingHeading = false): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let list: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length > 0) blocks.push({ kind: "paragraph", text: paragraph.join(" ") });
    paragraph = [];
  };
  const flushList = () => {
    if (list.length > 0) blocks.push({ kind: "list", items: list });
    list = [];
  };

  for (const rawLine of source.replace(/\r\n?/g, "\n").split("\n")) {
    const line = rawLine.trimEnd();
    if (line.trim() === "") {
      flushParagraph();
      flushList();
      continue;
    }
    const heading = HEADING.exec(line.trim());
    if (heading) {
      flushParagraph();
      flushList();
      blocks.push({ kind: "heading", text: heading[1] ?? "" });
      continue;
    }
    const bullet = BULLET.exec(line);
    if (bullet) {
      flushParagraph();
      list.push(bullet[1] ?? "");
      continue;
    }
    flushList();
    paragraph.push(line.trim());
  }
  flushParagraph();
  flushList();

  if (skipLeadingHeading && blocks[0]?.kind === "heading") blocks.shift();
  return blocks;
}

/** Bold, inline code and italic. Underscore emphasis is not read: identifiers use underscores. */
const INLINE = /(\*\*[^*]+\*\*|`[^`]+`|(?<![\w*])\*[^*\s][^*]*\*(?![\w*]))/;

function renderInline(text: string, keyPrefix: string): ReactNode[] {
  // split() with one capture group returns plain text at even indexes and matches at odd ones.
  return text.split(INLINE).map((part, i) => {
    if (i % 2 === 0) return part;
    const key = `${keyPrefix}-${i}`;
    if (part.startsWith("**")) {
      return (
        <strong key={key} className="font-semibold text-text-primary">
          {part.slice(2, -2)}
        </strong>
      );
    }
    if (part.startsWith("`")) {
      return (
        <code key={key} className="rounded bg-surface-2 px-1 font-mono text-[0.9em]">
          {part.slice(1, -1)}
        </code>
      );
    }
    return <em key={key}>{part.slice(1, -1)}</em>;
  });
}

type SafeMarkdownProps = {
  source: string;
  /** Drops a first heading line when the surrounding UI already shows the title. */
  skipLeadingHeading?: boolean;
  className?: string;
};

export function SafeMarkdown({ source, skipLeadingHeading = false, className }: SafeMarkdownProps) {
  const blocks = parseSafeMarkdown(source, skipLeadingHeading);
  return (
    <div className={cn("space-y-1.5 text-xs text-text-secondary", className)}>
      {blocks.map((block, i) => {
        const key = `b${i}`;
        if (block.kind === "heading") {
          return (
            <p key={key} className="pt-1 font-semibold text-text-primary">
              {renderInline(block.text, key)}
            </p>
          );
        }
        if (block.kind === "list") {
          return (
            <ul key={key} className="list-disc space-y-0.5 pl-4">
              {block.items.map((item, j) => (
                <li key={`${key}-${j}`}>{renderInline(item, `${key}-${j}`)}</li>
              ))}
            </ul>
          );
        }
        return <p key={key}>{renderInline(block.text, key)}</p>;
      })}
    </div>
  );
}
