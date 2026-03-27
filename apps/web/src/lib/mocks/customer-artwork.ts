export interface MockArtwork {
  id: string;
  name: string;
  type: string;
  uploadedAt: string;
  dimensions: string;
}

export const mockArtwork: MockArtwork[] = [
  {
    id: "a1",
    name: "Company Logo - Full Color",
    type: "Vector (AI)",
    uploadedAt: "2026-02-15",
    dimensions: '4" x 3"',
  },
  {
    id: "a2",
    name: "Event T-Shirt Design 2026",
    type: "Raster (PSD)",
    uploadedAt: "2026-03-01",
    dimensions: '12" x 14"',
  },
  {
    id: "a3",
    name: "Left Chest Logo",
    type: "Vector (SVG)",
    uploadedAt: "2026-01-20",
    dimensions: '3.5" x 3.5"',
  },
  {
    id: "a4",
    name: "Back Print - Team Roster",
    type: "Raster (PNG)",
    uploadedAt: "2026-03-10",
    dimensions: '10" x 14"',
  },
];
