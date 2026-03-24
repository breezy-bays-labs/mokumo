import type { Component } from "svelte";
import House from "@lucide/svelte/icons/house";
import Users from "@lucide/svelte/icons/users";
import FileText from "@lucide/svelte/icons/file-text";
import ShoppingCart from "@lucide/svelte/icons/shopping-cart";
import Receipt from "@lucide/svelte/icons/receipt";
import Palette from "@lucide/svelte/icons/palette";
import Settings from "@lucide/svelte/icons/settings";
import Factory from "@lucide/svelte/icons/factory";
import Truck from "@lucide/svelte/icons/truck";
import Shirt from "@lucide/svelte/icons/shirt";

export interface NavItem {
  title: string;
  url: string;
  icon: Component<Record<string, unknown>>;
  hidden?: boolean;
}

export const navItems: NavItem[] = [
  { title: "Home", url: "/", icon: House },
  { title: "Customers", url: "/customers", icon: Users },
  { title: "Quotes", url: "/quotes", icon: FileText },
  { title: "Orders", url: "/orders", icon: ShoppingCart },
  { title: "Invoices", url: "/invoices", icon: Receipt },
  { title: "Artwork", url: "/artwork", icon: Palette },
  { title: "Settings", url: "/settings", icon: Settings },
  { title: "Production", url: "/production", icon: Factory, hidden: true },
  { title: "Shipping", url: "/shipping", icon: Truck, hidden: true },
  { title: "Garments", url: "/garments", icon: Shirt, hidden: true },
];
