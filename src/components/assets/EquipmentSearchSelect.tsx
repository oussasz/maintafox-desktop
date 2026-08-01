/**
 * Searchable single-select for reference catalog options (class, family, criticality, status, org).
 * Radix Select does not filter — this uses a trigger button + filterable list panel.
 */

import { Check, ChevronsUpDown } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

export interface EquipmentSearchOption {
  value: string;
  label: string;
  /** Shown as secondary line (e.g. code) */
  description?: string;
}

interface EquipmentSearchSelectProps {
  id?: string;
  options: EquipmentSearchOption[];
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  emptyLabel?: string;
  disabled?: boolean;
  className?: string;
  "aria-invalid"?: boolean;
}

export function EquipmentSearchSelect({
  id,
  options,
  value,
  onChange,
  placeholder = "",
  emptyLabel = "—",
  disabled = false,
  className,
  "aria-invalid": ariaInvalid,
}: EquipmentSearchSelectProps) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  const selected = useMemo(() => options.find((o) => o.value === value) ?? null, [options, value]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return options;
    return options.filter((o) => {
      const hay = `${o.label} ${o.value} ${o.description ?? ""}`.toLowerCase();
      return hay.includes(q);
    });
  }, [options, query]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery("");
      }
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [open]);

  const displayLabel = selected
    ? selected.description
      ? `${selected.label} (${selected.description})`
      : selected.label
    : "";

  const handlePick = useCallback(
    (v: string) => {
      onChange(v);
      setOpen(false);
      setQuery("");
    },
    [onChange],
  );

  return (
    <div ref={rootRef} className={cn("relative w-full", className)}>
      <Button
        id={id}
        type="button"
        variant="outline"
        role="combobox"
        aria-expanded={open}
        aria-invalid={ariaInvalid}
        disabled={disabled}
        className={cn(
          "h-auto min-h-9 w-full justify-between font-normal px-3 py-2 text-left",
          !selected && "text-text-muted",
        )}
        onClick={() => !disabled && setOpen((o) => !o)}
      >
        <span className="truncate">{selected ? displayLabel : placeholder || emptyLabel}</span>
        <ChevronsUpDown className="ml-2 h-4 w-4 shrink-0 opacity-50" />
      </Button>

      {open && (
        <div className="absolute left-0 right-0 top-full z-50 mt-1 rounded-md border border-surface-border bg-surface-1 p-2 shadow-lg">
          <Input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={placeholder}
            className="mb-2 h-8 text-sm"
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setOpen(false);
                setQuery("");
              }
            }}
          />
          <ul className="max-h-60 overflow-auto rounded-sm border border-surface-border/80">
            {filtered.length === 0 ? (
              <li className="px-2 py-3 text-center text-xs text-text-muted">—</li>
            ) : (
              filtered.map((o) => (
                <li key={o.value}>
                  <button
                    type="button"
                    className={cn(
                      "flex w-full items-start gap-2 px-2 py-2 text-left text-sm hover:bg-muted",
                      o.value === value && "bg-muted/80",
                    )}
                    onClick={() => handlePick(o.value)}
                  >
                    <Check
                      className={cn(
                        "mt-0.5 h-4 w-4 shrink-0",
                        o.value === value ? "opacity-100" : "opacity-0",
                      )}
                    />
                    <span>
                      <span className="block font-medium leading-tight">{o.label}</span>
                      {o.description && (
                        <span className="text-[10px] font-mono text-text-muted">
                          {o.description}
                        </span>
                      )}
                    </span>
                  </button>
                </li>
              ))
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
