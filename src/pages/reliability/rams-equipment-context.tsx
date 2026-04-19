import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

const STORAGE_KEY = "maintafox.rams.selectedEquipmentId";
const STORAGE_MULTI_KEY = "maintafox.rams.selectedEquipmentIds";

export type RamsEquipmentContextValue = {
  selectedEquipmentId: number | null;
  selectedEquipmentIds: number[];
  setSelectedEquipmentId: (id: number | null) => void;
  setSelectedEquipmentIds: (ids: number[]) => void;
  toggleSelectedEquipmentId: (id: number, opts?: { exclusive?: boolean }) => void;
};

const RamsEquipmentContext = createContext<RamsEquipmentContextValue | null>(null);

function readStoredId(): number | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw == null || raw === "") {
      return null;
    }
    const n = Number.parseInt(raw, 10);
    return Number.isNaN(n) ? null : n;
  } catch {
    return null;
  }
}

function readStoredIds(): number[] {
  try {
    const raw = localStorage.getItem(STORAGE_MULTI_KEY);
    if (raw != null && raw.trim() !== "") {
      const parsed = JSON.parse(raw) as unknown;
      if (Array.isArray(parsed)) {
        return [
          ...new Set(parsed.map((v) => Number(v)).filter((n) => Number.isFinite(n) && n > 0)),
        ];
      }
    }
  } catch {
    /* ignore */
  }
  const legacy = readStoredId();
  return legacy == null ? [] : [legacy];
}

export function RamsEquipmentProvider({ children }: { children: ReactNode }) {
  const [selectedEquipmentIds, setSelectedEquipmentIdsState] = useState<number[]>(readStoredIds);

  const setSelectedEquipmentId = useCallback((id: number | null) => {
    const next = id == null ? [] : [id];
    setSelectedEquipmentIdsState(next);
    try {
      if (id == null) {
        localStorage.removeItem(STORAGE_KEY);
        localStorage.removeItem(STORAGE_MULTI_KEY);
      } else {
        localStorage.setItem(STORAGE_KEY, String(id));
        localStorage.setItem(STORAGE_MULTI_KEY, JSON.stringify(next));
      }
    } catch {
      /* ignore quota / private mode */
    }
  }, []);

  const setSelectedEquipmentIds = useCallback((ids: number[]) => {
    const next = [...new Set(ids.map((v) => Number(v)).filter((n) => Number.isFinite(n) && n > 0))];
    setSelectedEquipmentIdsState(next);
    try {
      if (next.length === 0) {
        localStorage.removeItem(STORAGE_KEY);
        localStorage.removeItem(STORAGE_MULTI_KEY);
      } else {
        localStorage.setItem(STORAGE_KEY, String(next[0]));
        localStorage.setItem(STORAGE_MULTI_KEY, JSON.stringify(next));
      }
    } catch {
      /* ignore quota / private mode */
    }
  }, []);

  const toggleSelectedEquipmentId = useCallback(
    (id: number, opts?: { exclusive?: boolean }) => {
      const parsedId = Number(id);
      if (!Number.isFinite(parsedId) || parsedId <= 0) {
        return;
      }
      if (opts?.exclusive) {
        setSelectedEquipmentIds([parsedId]);
        return;
      }
      setSelectedEquipmentIdsState((current) => {
        const has = current.includes(parsedId);
        const next = has ? current.filter((v) => v !== parsedId) : [...current, parsedId];
        try {
          if (next.length === 0) {
            localStorage.removeItem(STORAGE_KEY);
            localStorage.removeItem(STORAGE_MULTI_KEY);
          } else {
            localStorage.setItem(STORAGE_KEY, String(next[0]));
            localStorage.setItem(STORAGE_MULTI_KEY, JSON.stringify(next));
          }
        } catch {
          /* ignore */
        }
        return next;
      });
    },
    [setSelectedEquipmentIds],
  );

  const value = useMemo(
    () => ({
      selectedEquipmentId: selectedEquipmentIds[0] ?? null,
      selectedEquipmentIds,
      setSelectedEquipmentId,
      setSelectedEquipmentIds,
      toggleSelectedEquipmentId,
    }),
    [
      selectedEquipmentIds,
      setSelectedEquipmentId,
      setSelectedEquipmentIds,
      toggleSelectedEquipmentId,
    ],
  );

  return <RamsEquipmentContext.Provider value={value}>{children}</RamsEquipmentContext.Provider>;
}

export function useRamsEquipment(): RamsEquipmentContextValue {
  const ctx = useContext(RamsEquipmentContext);
  if (ctx == null) {
    throw new Error("useRamsEquipment must be used within RamsEquipmentProvider");
  }
  return ctx;
}

/** Use only under RAMS layout when an asset is required (main content is gated). */
export function useRequiredRamsEquipmentId(): number {
  const { selectedEquipmentId } = useRamsEquipment();
  if (selectedEquipmentId == null) {
    throw new Error("RAMS equipment is not selected");
  }
  return selectedEquipmentId;
}

export function useRequiredRamsEquipmentIds(): number[] {
  const { selectedEquipmentIds } = useRamsEquipment();
  if (selectedEquipmentIds.length === 0) {
    throw new Error("RAMS equipment is not selected");
  }
  return selectedEquipmentIds;
}
