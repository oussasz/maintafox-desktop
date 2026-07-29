import {
  LayoutDashboard,
  Cog,
  Building2,
  Wrench,
  ClipboardList,
  Users,
  Package,
  CalendarClock,
  Activity,
  BarChart3,
  Archive,
  BookOpen,
  Bell,
  HelpCircle,
  Calendar,
  ScrollText,
  Settings,
  User,
  GraduationCap,
  ShieldCheck,
  DollarSign,
  CheckSquare,
  HeartPulse,
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
    // Landing is /personnel?tab=training — gated by per.view (same as personnel route).
    path: "/personnel?tab=training",
    icon: <GraduationCap className="h-4 w-4" />,
    requiredPermission: P.PER_VIEW,
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
    key: "analytics",
    labelKey: "nav.analytics",
    path: "/analytics",
    icon: <BarChart3 className="h-4 w-4" />,
    requiredPermission: P.REP_VIEW,
  },
  {
    key: "reliability",
    labelKey: "nav.reliability",
    // Parent route PermissionRoute uses rep.view (shared analytics/reliability gate).
    path: "/reliability/dashboard",
    icon: <Activity className="h-4 w-4" />,
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
    // Route allows adm.users OR adm.roles (PermissionRoute anyOf). Sidebar only
    // supports a single requiredPermission — use adm.users for nav visibility.
    requiredPermission: P.ADM_USERS,
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
    key: "notifications",
    labelKey: "nav.notifications",
    path: "/notifications",
    icon: <Bell className="h-4 w-4" />,
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
    key: "activity",
    labelKey: "nav.activity",
    path: "/activity",
    icon: <ScrollText className="h-4 w-4" />,
    requiredPermission: P.LOG_VIEW,
  },
  {
    key: "settings",
    labelKey: "nav.settings",
    path: "/settings",
    icon: <Settings className="h-4 w-4" />,
    requiredPermission: P.ADM_SETTINGS,
  },
  {
    key: "diagnostics",
    labelKey: "nav.diagnostics",
    path: "/diagnostics",
    icon: <HeartPulse className="h-4 w-4" />,
    requiredPermission: P.ADM_SETTINGS,
  },
  { key: "profile", labelKey: "nav.profile", path: "/profile", icon: <User className="h-4 w-4" /> },
];

/** Flat route list for React Router (excludes group headers) */
export const appRoutes = defaultNavItems.filter((i) => !i.isGroupHeader);
