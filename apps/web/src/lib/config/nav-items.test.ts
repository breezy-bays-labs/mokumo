import { describe, it, expect } from "vitest";
import { navItems } from "./nav-items";

describe("navItems", () => {
  it("has exactly 10 items total", () => {
    expect(navItems).toHaveLength(10);
  });

  it("has 7 visible items", () => {
    const visible = navItems.filter((item) => !item.hidden);
    expect(visible).toHaveLength(7);
  });

  it("has 3 hidden items", () => {
    const hidden = navItems.filter((item) => item.hidden);
    expect(hidden).toHaveLength(3);
  });

  it("every item has a valid URL starting with /", () => {
    for (const item of navItems) {
      expect(item.url).toMatch(/^\//);
    }
  });

  it("every item has an icon component", () => {
    for (const item of navItems) {
      expect(item.icon).toBeDefined();
    }
  });

  it("Settings links to /settings (redirect handles sub-route)", () => {
    const settings = navItems.find((item) => item.title === "Settings");
    expect(settings).toBeDefined();
    expect(settings!.url).toBe("/settings");
  });

  it("visible items are in correct order", () => {
    const visible = navItems.filter((item) => !item.hidden);
    const titles = visible.map((item) => item.title);
    expect(titles).toEqual([
      "Home",
      "Customers",
      "Quotes",
      "Orders",
      "Invoices",
      "Artwork",
      "Settings",
    ]);
  });

  it("hidden items include Production, Shipping, Garments", () => {
    const hidden = navItems.filter((item) => item.hidden);
    const titles = hidden.map((item) => item.title);
    expect(titles).toContain("Production");
    expect(titles).toContain("Shipping");
    expect(titles).toContain("Garments");
  });

  it("every item has a unique URL", () => {
    const urls = navItems.map((item) => item.url);
    expect(new Set(urls).size).toBe(urls.length);
  });
});
