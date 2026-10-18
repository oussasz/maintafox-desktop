/**
 * "A list of Données de référence changed in this app": published, a value created, edited,
 * deactivated, reactivated, moved or merged, or an import applied. Screens that keep a copy of a
 * plant list (personnel lists, document categories) subscribe and read it again at once, so an
 * edit is visible without restarting. Changes received by sync are picked up on the next screen
 * mount instead (see `REFERENCE_LIST_MAX_AGE_MS`).
 */

/** A kept copy of a plant list older than this is read again when a screen using it opens. */
export const REFERENCE_LIST_MAX_AGE_MS = 5 * 60 * 1000;

type Listener = () => void;
const listeners = new Set<Listener>();

/** Subscribes to reference-data changes; returns the unsubscribe function. */
export function onReferenceDataChanged(listener: Listener): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Called by the reference service after a change succeeded. */
export function notifyReferenceDataChanged(): void {
  for (const listener of [...listeners]) {
    try {
      listener();
    } catch {
      // A failing subscriber must not break the save that succeeded nor the other subscribers.
    }
  }
}
