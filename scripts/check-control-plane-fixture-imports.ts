import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = process.cwd();
const SRC_DIR = join(ROOT, "src");

const SKIP_SEGMENTS = new Set(["node_modules", "dist", ".git", "coverage"]);
const TEST_PATH_MARKERS = ["__tests__", ".test.", ".spec.", "src/test/"];
const ALLOWED_IMPORT_MARKERS = ["/test/", "/tests/", "__tests__"];
const BLOCKED_IMPORT_PATTERNS = [
  /from\s+["'][^"']*fixtures\/mock-[^"']*["']/g,
  /import\s*\(\s*["'][^"']*fixtures\/mock-[^"']*["']\s*\)/g,
  /from\s+["'][^"']*mock-[^"']*["']/g,
  /import\s*\(\s*["'][^"']*mock-[^"']*["']\s*\)/g,
];

function shouldSkipPath(path: string): boolean {
  return path.split(/[/\\]/).some((segment) => SKIP_SEGMENTS.has(segment));
}

function isTestFile(path: string): boolean {
  const normalized = path.replaceAll("\\", "/");
  return TEST_PATH_MARKERS.some((marker) => normalized.includes(marker));
}

function walk(dir: string, out: string[]): void {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (shouldSkipPath(full)) continue;
    const st = statSync(full);
    if (st.isDirectory()) {
      walk(full, out);
      continue;
    }
    if (!full.endsWith(".ts") && !full.endsWith(".tsx")) continue;
    out.push(full);
  }
}

function main(): void {
  const files: string[] = [];
  walk(SRC_DIR, files);

  const violations: Array<{ file: string; fragment: string }> = [];

  for (const file of files) {
    if (isTestFile(file)) continue;
    const text = readFileSync(file, "utf8");
    for (const pattern of BLOCKED_IMPORT_PATTERNS) {
      const matches = text.match(pattern);
      if (!matches) continue;
      for (const fragment of matches) {
        const normalizedFragment = fragment.replaceAll("\\", "/");
        const isAllowed = ALLOWED_IMPORT_MARKERS.some((marker) => normalizedFragment.includes(marker));
        if (!isAllowed) {
          violations.push({
            file: relative(ROOT, file).replaceAll("\\", "/"),
            fragment,
          });
        }
      }
    }
  }

  if (violations.length === 0) {
    console.log("check-control-plane-fixture-imports: OK (no production fixture/mock imports found)");
    return;
  }

  console.error("check-control-plane-fixture-imports: found fixture/mock imports in production files:");
  for (const v of violations) {
    console.error(`- ${v.file}: ${v.fragment}`);
  }
  process.exitCode = 1;
}

main();
