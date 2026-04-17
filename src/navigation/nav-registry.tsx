import {
  LayoutDashboard,
  Cog,
  Building2,
  Wrench,
  ClipboardList,
  Users,
  UserCog,
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
  Radio,
  Link,
  ShieldCheck,
  DollarSign,
  CheckSquare,
  Sliders,
  HeartPulse,
} from "lucide-react";

import type { NavItem } from "@/components/layout/Sidebar";

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
    requiredPermission: "eq.view",
  },
  {
    key: "requests",
    labelKey: "nav.requests",
    path: "/requests",
    icon: <ClipboardList className="h-4 w-4" />,
    requiredPermission: "di.view",
  },
  {
    key: "work-orders",
    labelKey: "nav.workOrders",
    path: "/work-orders",
    icon: <Wrench className="h-4 w-4" />,
    requiredPermission: "ot.view",
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
    requiredPermission: "plan.view",
  },
  {
    key: "pm",
    labelKey: "nav.pm",
    path: "/pm",
    icon: <CalendarClock className="h-4 w-4" />,
    requiredPermission: "pm.view",
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
    requiredPermission: "ptw.view",
  },
  {
    key: "inspections",
    labelKey: "nav.inspections",
    path: "/inspections",
    icon: <CheckSquare className="h-4 w-4" />,
    requiredPermission: "ins.view",
  },
  {
    key: "training",
    labelKey: "nav.training",
    path: "/training",
    icon: <GraduationCap className="h-4 w-4" />,
    requiredPermission: "trn.view",
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
    requiredPermission: "inv.view",
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
    requiredPermission: "rep.view",
  },
  {
    key: "reliability",
    labelKey: "nav.reliability",
    path: "/reliability",
    icon: <Activity className="h-4 w-4" />,
    requiredPermission: "ram.view",
  },
  {
    key: "budget",
    labelKey: "nav.budget",
    path: "/budget",
    icon: <DollarSign className="h-4 w-4" />,
    requiredPermission: "fin.view",
  },

  // ── Administration ────────────────────────────────────────
  { key: "g-admin", labelKey: "nav.groups.admin", path: "#", icon: null, isGroupHeader: true },
  {
    key: "personnel",
    labelKey: "nav.personnel",
    path: "/personnel",
    icon: <Users className="h-4 w-4" />,
    requiredPermission: "per.view",
  },
  {
    key: "users",
    labelKey: "nav.users",
    path: "/users",
    icon: <UserCog className="h-4 w-4" />,
    requiredPermission: "adm.users",
  },
  {
    key: "org",
    labelKey: "nav.org",
    path: "/org",
    icon: <Building2 className="h-4 w-4" />,
    requiredPermission: "org.view",
  },
  {
    key: "lookups",
    labelKey: "nav.lookups",
    path: "/lookups",
    icon: <BookOpen className="h-4 w-4" />,
    requiredPermission: "ref.view",
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
    requiredPermission: "doc.view",
  },
  {
    key: "iot",
    labelKey: "nav.iot",
    path: "/iot",
    icon: <Radio className="h-4 w-4" />,
    requiredPermission: "iot.view",
  },
  {
    key: "erp",
    labelKey: "nav.erp",
    path: "/erp",
    icon: <Link className="h-4 w-4" />,
    requiredPermission: "erp.view",
  },
  {
    key: "archive",
    labelKey: "nav.archive",
    path: "/archive",
    icon: <Archive className="h-4 w-4" />,
    requiredPermission: "arc.view",
  },
  {
    key: "activity",
    labelKey: "nav.activity",
    path: "/activity",
    icon: <ScrollText className="h-4 w-4" />,
    requiredPermission: "log.view",
  },
  {
    key: "settings",
    labelKey: "nav.settings",
    path: "/settings",
    icon: <Settings className="h-4 w-4" />,
    requiredPermission: "adm.settings",
  },
  {
    key: "configuration",
    labelKey: "nav.configuration",
    path: "/configuration",
    icon: <Sliders className="h-4 w-4" />,
    requiredPermission: "cfg.view",
  },
  {
    key: "diagnostics",
    labelKey: "nav.diagnostics",
    path: "/diagnostics",
    icon: <HeartPulse className="h-4 w-4" />,
    requiredPermission: "adm.settings",
  },
  { key: "profile", labelKey: "nav.profile", path: "/profile", icon: <User className="h-4 w-4" /> },
];

/** Flat route list for React Router (excludes group headers) */
export const appRoutes = defaultNavItems.filter((i) => !i.isGroupHeader);
