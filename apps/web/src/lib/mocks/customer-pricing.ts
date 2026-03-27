export interface MockPricingTemplate {
  id: string;
  name: string;
  method: string;
  basePrice: string;
  discount: string;
}

export const mockPricingTemplates: MockPricingTemplate[] = [
  {
    id: "p1",
    name: "Standard Screen Print",
    method: "Screen Print",
    basePrice: "$8.50/unit",
    discount: "10% volume (100+)",
  },
  {
    id: "p2",
    name: "DTG Premium",
    method: "Direct to Garment",
    basePrice: "$15.00/unit",
    discount: "None",
  },
  {
    id: "p3",
    name: "Embroidery - Left Chest",
    method: "Embroidery",
    basePrice: "$6.00/unit",
    discount: "15% repeat order",
  },
];
