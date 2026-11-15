import { useMediaQuery } from "@/hooks/useMediaQuery";
import { useAppStore } from "@/store/app-store";

/**
 * The menu shows icons only when the user folded it, or when the window is narrower than
 * 1200 px (760–1199 px laptops and split screens): the page keeps its width (L6-20). The
 * user's own choice is kept for wide windows.
 */
export function useSidebarFolded(): boolean {
  const collapsed = useAppStore((s) => s.sidebarCollapsed);
  const wide = useMediaQuery("(min-width: 1200px)", true);
  return collapsed || !wide;
}
