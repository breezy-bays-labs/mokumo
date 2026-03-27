/**
 * Module-level state for breadcrumb label overrides.
 * Child layouts can set a human-readable label for dynamic URL segments
 * (e.g., replacing a UUID with a customer name).
 */
const overrides = $state<Record<string, string>>({});

export function setBreadcrumbLabel(segment: string, label: string): void {
  overrides[segment] = label;
}

export function clearBreadcrumbLabel(segment: string): void {
  delete overrides[segment];
}

export function getBreadcrumbLabel(segment: string): string | undefined {
  return overrides[segment];
}
