import { SvelteSet } from "svelte/reactivity";

/**
 * Profile-switching UI state, shared across components.
 *
 * Uses a reactive $state object so mutations are visible to all importers
 * without needing separate setter functions (direct export of reassigned
 * $state primitives is not reactive across module boundaries in Svelte 5).
 */
export const profile = $state({
  /** Whether the profile switcher dropdown is open. */
  openProfileSwitcher: false,
  /** The profile the user is switching toward (set before confirmation dialogs). */
  switchTarget: null as "demo" | "production" | null,
  /**
   * Set of form IDs that have unsaved changes.
   * Used by the dirty-form guard to prompt before a profile switch.
   */
  dirtyForms: new SvelteSet<string>(),
  /**
   * Whether the unsaved changes confirmation dialog is open.
   * Set to true when a profile switch is blocked by dirty forms.
   */
  profileSwitchPending: false,
});
