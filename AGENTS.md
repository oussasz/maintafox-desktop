# Agent instructions

## Rust build cache (disk space)

- Always build into the default `src-tauri/target`. Do **not** set `CARGO_TARGET_DIR`
  or create `target-*` / `target/<something>-check` directories — each one is a full
  2–15 GB copy of the build and they were never cleaned up (they once filled ~90 GB).
- If cargo reports "Blocking waiting for file lock on build directory", wait for the
  other build; do not work around it with a separate target dir.
- To trim the cache: `npm run clean:cache` (`--dry-run` to preview). It only removes
  build output cargo regenerates; it refuses to run while cargo is running.
