import { beforeNavigate, goto } from "$app/navigation";
import { profile } from "$lib/stores/profile.svelte";

/**
 * Install the unsaved-changes navigation guard on the current layout.
 * Call once at the top level of each layout's `<script>` block.
 *
 * When dirty forms exist and the user tries to navigate away,
 * the navigation is cancelled and a confirmation dialog is shown.
 */
export function installNavigationGuard(): void {
  beforeNavigate((navigation) => {
    const { cancel, to, willUnload, type } = navigation;
    if (willUnload) return;

    if (profile.dirtyForms.size > 0 && !profile.unsavedChangesDialogOpen) {
      cancel();
      const delta =
        type === "popstate" && "delta" in navigation ? (navigation.delta as number) : undefined;
      profile.pendingNavigation = to?.url.href ? { href: to.url.href, delta } : null;
      profile.unsavedChangesDialogOpen = true;
      return;
    }

    if (profile.unsavedChangesDialogOpen) {
      cancel();
    }
  });
}

/**
 * Replay a previously cancelled navigation.
 * Uses `history.go(delta)` for back/forward (popstate) to preserve
 * browser history semantics, and `goto(href)` for link clicks.
 */
export async function replayNavigation(
  pending: { href: string; delta?: number } | null,
): Promise<void> {
  if (!pending) return;
  try {
    if (pending.delta !== undefined) {
      history.go(pending.delta);
    } else {
      await goto(pending.href);
    }
  } catch (error) {
    console.error("Navigation replay failed:", error);
    window.location.assign(pending.href);
  }
}
