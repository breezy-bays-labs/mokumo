import { describe, it, expect } from "vitest";
import { formatCurrency } from "./format";

describe("formatCurrency", () => {
  it("formats cents to USD currency string", () => {
    expect(formatCurrency(5000)).toBe("$50.00");
  });

  it("formats zero cents", () => {
    expect(formatCurrency(0)).toBe("$0.00");
  });

  it("formats small amounts correctly", () => {
    expect(formatCurrency(1)).toBe("$0.01");
  });

  it("formats large amounts with commas", () => {
    expect(formatCurrency(1000000)).toBe("$10,000.00");
  });

  it("returns em dash for null", () => {
    expect(formatCurrency(null)).toBe("—");
  });

  it("returns em dash for undefined", () => {
    // The function checks for both null and undefined despite the type signature
    expect(formatCurrency(undefined as unknown as null)).toBe("—");
  });

  it("handles negative cents", () => {
    expect(formatCurrency(-500)).toBe("-$5.00");
  });

  it("formats fractional cent values", () => {
    // 1050 cents = $10.50
    expect(formatCurrency(1050)).toBe("$10.50");
  });
});
