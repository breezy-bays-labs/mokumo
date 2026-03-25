import { toast, Toaster } from "svelte-sonner";

/**
 * OKLCH-based toast classes for unstyled svelte-sonner.
 * The `toast` key provides base layout since `unstyled: true` strips all defaults.
 * Uses border-emphasis + foreground text for cross-theme legibility.
 */
export const toastClasses = {
  toast:
    "group relative flex items-center gap-2 rounded-lg px-4 py-3 pr-8 shadow-lg border bg-background text-foreground",
  success: "border-success bg-success/10 text-foreground",
  error: "border-error bg-error/10 text-foreground",
  warning: "border-warning bg-warning/10 text-foreground",
  info: "border-border bg-muted text-foreground",
  closeButton:
    "absolute right-1 top-1 !h-5 !w-5 flex items-center justify-center !rounded-full !border !border-border !bg-background !text-foreground/50 hover:!text-foreground hover:!bg-muted transition-colors",
};

export { toast, Toaster };
