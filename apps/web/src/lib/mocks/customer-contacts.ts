export interface MockContact {
  id: string;
  name: string;
  role: string;
  email: string;
  phone: string;
  isPrimary: boolean;
}

export const mockContacts: MockContact[] = [
  {
    id: "c1",
    name: "Sarah Johnson",
    role: "Owner",
    email: "sarah@acmeprinting.com",
    phone: "(555) 123-4567",
    isPrimary: true,
  },
  {
    id: "c2",
    name: "Mike Rodriguez",
    role: "Production Manager",
    email: "mike@acmeprinting.com",
    phone: "(555) 234-5678",
    isPrimary: false,
  },
  {
    id: "c3",
    name: "Emily Chen",
    role: "Accounts Payable",
    email: "ap@acmeprinting.com",
    phone: "(555) 345-6789",
    isPrimary: false,
  },
];
