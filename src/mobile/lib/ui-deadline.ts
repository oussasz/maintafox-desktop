// A screen never waits forever on the engine. The engine's own licence calls give up after 20 s
// (sync/transport/client.rs, ClientTimeouts::interactive), but an engine panic or a lost IPC answer
// leaves the promise pending: the screen then gives up on its own and offers to retry.

/** Longer than the engine's 20 s request limit, so a normal engine answer always comes first. */
export const UI_DEADLINE_MS = 25_000;

/**
 * Local-only steps on the phone (clearing a previous plant's data, recording the licence): no
 * network, but they can take long on a slow phone, and a retry must not start while one runs.
 */
export const LOCAL_STEP_DEADLINE_MS = 120_000;

/** After this long the screen says the server is slow and offers to stop waiting. */
export const SLOW_HINT_MS = 10_000;

/**
 * The engine's answer, or after `ms` a refusal classified as "server unreachable" (code
 * `interrupted`, see user-error.ts). The engine call itself is not cancelled: its late answer is
 * ignored.
 */
export function withUiDeadline<T>(
  promise: Promise<T>,
  where: string,
  ms: number = UI_DEADLINE_MS,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<never>((_, reject) => {
    timer = setTimeout(
      () =>
        reject({
          code: "interrupted",
          message: `${where}: no answer from the engine after ${ms} ms`,
        }),
      ms,
    );
  });
  return Promise.race([promise, deadline]).finally(() => clearTimeout(timer));
}
