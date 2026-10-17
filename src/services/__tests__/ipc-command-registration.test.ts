/**
 * Contract test: every backend command a service calls is registered in the Tauri app.
 *
 * Each exported service function is called with a few argument shapes until it reaches the
 * IPC layer; the command it invokes must appear in `tauri::generate_handler![…]` in
 * src-tauri/src/lib.rs. A service calling an unregistered command is a dead button in the UI
 * (the backend answers "command not found").
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it, vi } from "vitest";

import type * as IpcInvoke from "@/lib/ipc-invoke";

const calls = vi.hoisted(() => ({ commands: [] as string[] }));

vi.mock("@/lib/ipc-invoke", async (importOriginal) => {
  const actual = await importOriginal<typeof IpcInvoke>();
  const record = (cmd: string): Promise<null> => {
    calls.commands.push(cmd);
    return Promise.resolve(null);
  };
  return { ...actual, invoke: vi.fn(record), invokeSilent: vi.fn(record) };
});

function registeredCommands(): Set<string> {
  const lib = readFileSync(resolve(__dirname, "../../../src-tauri/src/lib.rs"), "utf8");
  const start = lib.indexOf("generate_handler![");
  expect(start, "generate_handler! block in lib.rs").toBeGreaterThan(-1);
  const end = lib.indexOf("])", start);
  const block = lib.slice(start, end);
  const names = new Set<string>();
  for (const m of block.matchAll(/(?:::|\[|\s)([a-z_][a-z0-9_]*)\s*,/g)) {
    if (m[1]) names.add(m[1]);
  }
  return names;
}

const ARG_SHAPES: unknown[][] = [[], [1], [1, 1], [1, 1, 1], ["CODE"], [{}], [1, {}], [{ id: 1 }]];

async function settle(p: unknown): Promise<void> {
  if (!(p instanceof Promise)) return;
  await Promise.race([p.catch(() => undefined), new Promise((r) => setTimeout(r, 50))]);
}

const services = import.meta.glob<Record<string, unknown>>("../*-service.ts", { eager: true });

describe("IPC command registration", () => {
  it("every command invoked by a service is registered in lib.rs", async () => {
    const registered = registeredCommands();
    const invoked = new Map<string, string>();
    const unreached: string[] = [];

    for (const [path, mod] of Object.entries(services)) {
      for (const [name, fn] of Object.entries(mod)) {
        if (typeof fn !== "function" || /^[A-Z]/.test(name)) continue;
        let reached = false;
        for (const args of ARG_SHAPES) {
          calls.commands.length = 0;
          try {
            await settle((fn as (...a: unknown[]) => unknown)(...args));
          } catch {
            // Input validation can reject an argument shape before the IPC call; try the next.
          }
          if (calls.commands.length > 0) {
            for (const cmd of calls.commands) invoked.set(cmd, `${path}#${name}`);
            reached = true;
            break;
          }
        }
        if (!reached) unreached.push(`${path}#${name}`);
      }
    }

    const missing = [...invoked].filter(([cmd]) => !registered.has(cmd));
    expect(missing, "service commands missing from generate_handler!").toEqual([]);
    // Sanity: the harness really reached the IPC layer for most of the API surface.
    expect(invoked.size).toBeGreaterThan(300);
    // Pure helpers (formatters, schema mappers) never reach IPC; listed for diagnosis only.
    expect(unreached.length).toBeLessThan(invoked.size);
  }, 120_000);
});
