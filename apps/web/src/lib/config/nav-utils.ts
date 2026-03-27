import { getBreadcrumbLabel } from "./breadcrumb-overrides.svelte";
import { navItems } from "./nav-items";

export function isActive(url: string, pathname: string): boolean {
  if (url === "/") return pathname === "/";
  return pathname.startsWith(url);
}

const titleBySlug = new Map(
  navItems.map((item) => [item.url.split("/").filter(Boolean)[0] ?? "", item.title]),
);

export function labelForSlug(slug: string): string {
  return (
    getBreadcrumbLabel(slug) ??
    titleBySlug.get(slug) ??
    slug.charAt(0).toUpperCase() + slug.slice(1)
  );
}

export interface BreadcrumbSegment {
  label: string;
  href: string;
}

export function buildBreadcrumbs(pathname: string): BreadcrumbSegment[] {
  if (pathname === "/") return [{ label: "Home", href: "/" }];

  const parts = pathname.split("/").filter(Boolean);
  return [
    { label: "Home", href: "/" },
    ...parts.map((part, i) => ({
      label: labelForSlug(part),
      href: "/" + parts.slice(0, i + 1).join("/"),
    })),
  ];
}
