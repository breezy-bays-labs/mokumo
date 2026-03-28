import { describe, expect, it } from "vitest";
import { createCustomer, createHeroCustomer, seedCustomers } from "./customer";

describe("createCustomer", () => {
  it("always includes display_name", () => {
    const customer = createCustomer();
    expect(customer.display_name).toBeTruthy();
  });

  it("applies overrides", () => {
    const customer = createCustomer({ display_name: "Override Name", email: "test@test.com" });
    expect(customer.display_name).toBe("Override Name");
    expect(customer.email).toBe("test@test.com");
  });
});

describe("createHeroCustomer", () => {
  it("returns Gary Thompson for index 0", () => {
    const hero = createHeroCustomer(0);
    expect(hero.display_name).toBe("Gary Thompson");
    expect(hero.company_name).toBe("4Ink Custom Prints");
    expect(hero.tags).toContain("screen-print");
  });

  it("returns Sarah Chen for index 1", () => {
    const hero = createHeroCustomer(1);
    expect(hero.display_name).toBe("Sarah Chen");
    expect(hero.company_name).toBe("Threadworks Studio");
    expect(hero.tags).toContain("embroidery");
  });

  it("returns a copy, not the original object", () => {
    const a = createHeroCustomer(0);
    const b = createHeroCustomer(0);
    expect(a).toEqual(b);
    expect(a).not.toBe(b);
  });
});

describe("seedCustomers", () => {
  it("returns the requested number of customers", () => {
    const customers = seedCustomers(10, 42);
    expect(customers).toHaveLength(10);
  });

  it("places hero customers first", () => {
    const customers = seedCustomers(5, 42);
    expect(customers[0].display_name).toBe("Gary Thompson");
    expect(customers[1].display_name).toBe("Sarah Chen");
  });

  it("produces deterministic output with the same seed", () => {
    const a = seedCustomers(10, 123);
    const b = seedCustomers(10, 123);
    expect(a).toEqual(b);
  });

  it("produces different output with different seeds", () => {
    const a = seedCustomers(10, 100);
    const b = seedCustomers(10, 200);
    // Heroes are the same, but random customers should differ
    const randomA = a.slice(2).map((c) => c.display_name);
    const randomB = b.slice(2).map((c) => c.display_name);
    expect(randomA).not.toEqual(randomB);
  });

  it("returns exactly 2 heroes when count is 2", () => {
    const customers = seedCustomers(2, 42);
    expect(customers).toHaveLength(2);
    expect(customers[0].display_name).toBe("Gary Thompson");
    expect(customers[1].display_name).toBe("Sarah Chen");
  });

  it("throws if count < 2", () => {
    expect(() => seedCustomers(1)).toThrow("count >= 2");
  });
});
