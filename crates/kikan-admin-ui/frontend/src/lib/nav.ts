import type { Component } from "svelte";
import House from "@lucide/svelte/icons/house";
import Building2 from "@lucide/svelte/icons/building-2";
import Users from "@lucide/svelte/icons/users";
import Puzzle from "@lucide/svelte/icons/puzzle";
import Plug from "@lucide/svelte/icons/plug";
import Database from "@lucide/svelte/icons/database";
import Stethoscope from "@lucide/svelte/icons/stethoscope";
import Activity from "@lucide/svelte/icons/activity";
import Palette from "@lucide/svelte/icons/palette";
import Wifi from "@lucide/svelte/icons/wifi";
import RefreshCw from "@lucide/svelte/icons/refresh-cw";
import LifeBuoy from "@lucide/svelte/icons/life-buoy";
import Receipt from "@lucide/svelte/icons/receipt";
import Settings from "@lucide/svelte/icons/settings";

export type NavGroup = "TOP" | "PROFILE";

export interface NavEntry {
  id: string;
  label: string;
  /** Path relative to `kit.paths.base`. The sidebar prepends `base` at render time. */
  path: string;
  icon: Component<Record<string, unknown>>;
  group: NavGroup;
}

/**
 * 14-route admin nav manifest (CAO-A2).
 *
 * Pre-declared so PR 2B / PR 3 / PR 4 / PR 5 can fill page bodies without
 * touching the layout or sidebar. Order in this list is the render order in
 * the sidebar.
 */
export const navEntries: NavEntry[] = [
  { id: "overview", label: "Overview", path: "/", icon: House, group: "TOP" },
  { id: "profiles", label: "Profiles", path: "/profiles", icon: Building2, group: "TOP" },
  { id: "users", label: "Users", path: "/users", icon: Users, group: "TOP" },
  { id: "extensions", label: "Extensions", path: "/extensions", icon: Puzzle, group: "TOP" },
  { id: "integrations", label: "Integrations", path: "/integrations", icon: Plug, group: "TOP" },
  { id: "backup", label: "Backup", path: "/backup", icon: Database, group: "TOP" },
  { id: "diagnostics", label: "Diagnostics", path: "/diagnostics", icon: Stethoscope, group: "TOP" },
  { id: "activity", label: "Activity", path: "/activity", icon: Activity, group: "TOP" },
  { id: "appearance", label: "Appearance", path: "/appearance", icon: Palette, group: "TOP" },
  { id: "networking", label: "Networking", path: "/networking", icon: Wifi, group: "TOP" },
  { id: "updates", label: "Updates", path: "/updates", icon: RefreshCw, group: "TOP" },
  { id: "help", label: "Help", path: "/help", icon: LifeBuoy, group: "TOP" },
  { id: "billing", label: "Billing", path: "/billing", icon: Receipt, group: "TOP" },
  { id: "profile-settings", label: "Profile settings", path: "/profile/settings", icon: Settings, group: "PROFILE" },
];

export function isActive(currentPath: string, entry: NavEntry, base: string): boolean {
  if (entry.path === "/") {
    return currentPath === base || currentPath === `${base}/`;
  }
  const href = `${base}${entry.path}`;
  return currentPath === href || currentPath.startsWith(`${href}/`);
}
