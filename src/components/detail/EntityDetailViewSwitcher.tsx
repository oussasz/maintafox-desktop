import { Button } from "@/components/ui/button";

export interface EntityDetailViewTab {
  id: string;
  label: string;
}

export interface EntityDetailViewSwitcherProps {
  tabs: EntityDetailViewTab[];
  activeId: string;
  onChange: (id: string) => void;
}

/** Compact button switcher for Detail Workspace views. */
export function EntityDetailViewSwitcher({
  tabs,
  activeId,
  onChange,
}: EntityDetailViewSwitcherProps) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      {tabs.map((tab) => (
        <Button
          key={tab.id}
          type="button"
          size="sm"
          className="h-7 px-3 text-xs"
          variant={activeId === tab.id ? "default" : "outline"}
          onClick={() => onChange(tab.id)}
        >
          {tab.label}
        </Button>
      ))}
    </div>
  );
}
