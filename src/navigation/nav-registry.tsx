import {
  Activity,
  LayoutDashboard,
  Cog,
  Building2,
  Wrench,
  ClipboardList,
  Users,
  Package,
  CalendarClock,
  BarChart3,
  Archive,
  BookOpen,
  HelpCircle,
  Calendar,
  Settings,
  GraduationCap,
  ShieldCheck,
  DollarSign,
  CheckSquare,
} from "lucide-react";

import type { NavItem } from "@/components/layout/Sidebar";
import { P } from "@shared/rbac/permissions.generated";

export const defaultNavItems: NavItem[] = [
  // ── Core Operations ───────────────────────────────────────
  { key: "g-core", labelKey: "nav.groups.core", path: "#", icon: null, isGroupHeader: true },
  {
    key: "dashboard",
    labelKey: "nav.dashboard",
    path: "/",
    icon: <LayoutDashboard className="h-4 w-4" />,
  },
  {
    key: "equipment",
    labelKey: "nav.equipment",
    path: "/equipment",
    icon: <Cog className="h-4 w-4" />,
    requiredPermission: P.EQ_VIEW,
  },
  {
    key: "requests",
    labelKey: "nav.requests",
    path: "/requests",
    icon: <ClipboardList className="h-4 w-4" />,
    requiredPermission: P.DI_VIEW,
  },
  {
    key: "work-orders",
    labelKey: "nav.workOrders",
    path: "/work-orders",
    icon: <Wrench className="h-4 w-4" />,
    requiredPermission: P.OT_VIEW,
  },

  // ── Planning ──────────────────────────────────────────────
  {
    key: "g-planning",
    labelKey: "nav.groups.planning",
    path: "#",
    icon: null,
    isGroupHeader: true,
  },
  {
    key: "planning",
    labelKey: "nav.planning",
    path: "/planning",
    icon: <Calendar className="h-4 w-4" />,
    requiredPermission: P.PLAN_VIEW,
  },
  {
    key: "pm",
    labelKey: "nav.pm",
    path: "/pm",
    icon: <CalendarClock className="h-4 w-4" />,
    requiredPermission: P.PM_VIEW,
  },

  // ── Compliance ────────────────────────────────────────────
  {
    key: "g-compliance",
    labelKey: "nav.groups.compliance",
    path: "#",
    icon: null,
    isGroupHeader: true,
  },
  {
    key: "permits",
    labelKey: "nav.permits",
    path: "/permits",
    icon: <ShieldCheck className="h-4 w-4" />,
    requiredPermission: P.PTW_VIEW,
  },
  {
    key: "inspections",
    labelKey: "nav.inspections",
    path: "/inspections",
    icon: <CheckSquare className="h-4 w-4" />,
    requiredPermission: P.INS_VIEW,
  },
  {
    key: "training",
    labelKey: "nav.training",
    path: "/training",
    icon: <GraduationCap className="h-4 w-4" />,
    requiredPermission: P.TRN_VIEW,
  },

  // ── Inventory ─────────────────────────────────────────────
  {
    key: "g-inventory",
    labelKey: "nav.groups.inventory",
    path: "#",
    icon: null,
    isGroupHeader: true,
  },
  {
    key: "inventory",
    labelKey: "nav.inventory",
    path: "/inventory",
    icon: <Package className="h-4 w-4" />,
    requiredPermission: P.INV_VIEW,
  },

  // ── Analytics & Reporting ─────────────────────────────────
  {
    key: "g-analytics",
    labelKey: "nav.groups.analytics",
    path: "#",
    icon: null,
    isGroupHeader: true,
  },
  {
    key: "reliability",
    labelKey: "nav.reliability",
    path: "/reliability",
    icon: <Activity className="h-4 w-4" />,
    requiredPermission: P.RAM_VIEW,
  },
  {
    key: "analytics",
    labelKey: "nav.analytics",
    path: "/analytics",
    icon: <BarChart3 className="h-4 w-4" />,
    requiredPermission: P.REP_VIEW,
  },
  {
    key: "budget",
    labelKey: "nav.budget",
    path: "/budget",
    icon: <DollarSign className="h-4 w-4" />,
    requiredPermission: P.FIN_VIEW,
  },

  // ── Administration ────────────────────────────────────────
  { key: "g-admin", labelKey: "nav.groups.admin", path: "#", icon: null, isGroupHeader: true },
  {
    key: "personnel",
    labelKey: "nav.personnel",
    path: "/personnel",
    icon: <Users className="h-4 w-4" />,
    requiredPermission: P.PER_VIEW,
  },
  {
    key: "admin",
    labelKey: "nav.admin",
    path: "/admin",
    icon: <ShieldCheck className="h-4 w-4" />,
    // Route allows adm.users, adm.roles or adm.permissions (PermissionRoute anyOf): the entry
    // follows the same rule (the Permissions tab is reachable with adm.permissions alone).
    requiredPermission: P.ADM_USERS,
    alsoVisibleWith: [P.ADM_ROLES, P.ADM_PERMISSIONS],
  },
  {
    key: "org",
    labelKey: "nav.org",
    path: "/org",
    icon: <Building2 className="h-4 w-4" />,
    requiredPermission: P.ORG_VIEW,
  },
  {
    key: "lookups",
    labelKey: "nav.lookups",
    path: "/lookups",
    icon: <BookOpen className="h-4 w-4" />,
    requiredPermission: P.REF_VIEW,
  },
  {
    key: "documentation",
    labelKey: "nav.documentation",
    path: "/documentation",
    icon: <HelpCircle className="h-4 w-4" />,
    requiredPermission: P.DOC_VIEW,
  },
  {
    key: "archive",
    labelKey: "nav.archive",
    path: "/archive",
    icon: <Archive className="h-4 w-4" />,
    requiredPermission: P.ARC_VIEW,
  },
  {
    key: "settings",
    labelKey: "nav.settings",
    path: "/settings",
    icon: <Settings className="h-4 w-4" />,
  },
];

/** Flat route list for React Router (excludes group headers) */
export const appRoutes = defaultNavItems.filter((i) => !i.isGroupHeader);
