import { describe, it, expect } from "vitest";
import { isActive, labelForSlug, buildBreadcrumbs } from "./nav-utils";

describe("isActive", () => {
  it("root matches root exactly", () => {
    expect(isActive("/", "/")).toBe(true);
  });

  it("root does not match other routes", () => {
    expect(isActive("/", "/customers")).toBe(false);
  });

  it("non-root matches child routes", () => {
    expect(isActive("/customers", "/customers/123")).toBe(true);
  });

  it("non-root does not match unrelated routes", () => {
    expect(isActive("/orders", "/customers")).toBe(false);
  });
});

describe("labelForSlug", () => {
  it("returns nav item title for known slugs", () => {
    expect(labelForSlug("customers")).toBe("Customers");
  });

  it("capitalizes unknown slugs as fallback", () => {
    expect(labelForSlug("unknown")).toBe("Unknown");
  });
});

describe("buildBreadcrumbs", () => {
  it("returns Home for root", () => {
    expect(buildBreadcrumbs("/")).toEqual([{ label: "Home", href: "/" }]);
  });

  it("returns single segment for top-level route", () => {
    expect(buildBreadcrumbs("/customers")).toEqual([{ label: "Customers", href: "/customers" }]);
  });

  it("returns multiple segments for nested route", () => {
    expect(buildBreadcrumbs("/settings/shop")).toEqual([
      { label: "Settings", href: "/settings" },
      { label: "Shop", href: "/settings/shop" },
    ]);
  });
});
