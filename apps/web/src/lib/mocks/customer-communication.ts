export interface MockMessage {
  id: string;
  direction: "inbound" | "outbound";
  channel: string;
  subject: string;
  preview: string;
  timestamp: string;
}

export const mockMessages: MockMessage[] = [
  {
    id: "m1",
    direction: "outbound",
    channel: "Email",
    subject: "Quote #1042 - Spring Collection",
    preview: "Hi Sarah, please find attached the quote for your spring collection order...",
    timestamp: "2026-03-25T14:30:00Z",
  },
  {
    id: "m2",
    direction: "inbound",
    channel: "Email",
    subject: "Re: Quote #1042 - Spring Collection",
    preview: "Looks good! Can we adjust the qty on the polo shirts to 150?",
    timestamp: "2026-03-25T16:45:00Z",
  },
  {
    id: "m3",
    direction: "outbound",
    channel: "Email",
    subject: "Invoice #2089",
    preview: "Please find attached your invoice for the completed order...",
    timestamp: "2026-03-20T09:00:00Z",
  },
  {
    id: "m4",
    direction: "inbound",
    channel: "Portal",
    subject: "Artwork uploaded",
    preview: "Customer uploaded new artwork file: event-tshirt-2026-v2.ai",
    timestamp: "2026-03-18T11:20:00Z",
  },
];
