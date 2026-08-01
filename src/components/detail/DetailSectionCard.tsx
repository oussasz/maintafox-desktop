import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

export interface DetailSectionCardProps {
  title: string;
  icon?: LucideIcon;
  children: ReactNode;
  id?: string;
  className?: string;
}

/** Section card chrome matching Asset Details density. */
export function DetailSectionCard({
  title,
  icon: Icon,
  children,
  id,
  className,
}: DetailSectionCardProps) {
  return (
    <Card id={id} className={className}>
      <CardHeader className="pb-3">
        <div className="flex items-center gap-2">
          {Icon ? <Icon className="h-4 w-4 text-text-muted" aria-hidden /> : null}
          <CardTitle className="text-base">{title}</CardTitle>
        </div>
      </CardHeader>
      <CardContent className="space-y-2 text-sm">{children}</CardContent>
    </Card>
  );
}
