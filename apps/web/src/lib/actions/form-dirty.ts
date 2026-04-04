import { profile } from "$lib/stores/profile.svelte";
import type { Action } from "svelte/action";

let nextId = 0;

/**
 * Svelte action that tracks whether a form has unsaved changes.
 *
 * Apply to any `<form>` element: `<form use:formDirty>`.
 * On `input`, the form is marked dirty. The form is marked clean only when
 * it unmounts (destroy), which fires when the form closes on successful save
 * or when the user navigates away. This ensures a failed API save does not
 * silently discard the dirty state while the form is still open.
 *
 * @example
 * ```svelte
 * <form use:formDirty onsubmit={handleSubmit}>
 *   <input bind:value={name} />
 * </form>
 * ```
 */
export const formDirty: Action<HTMLFormElement> = (node) => {
  const id = `form-dirty-${nextId++}`;

  function markDirty() {
    profile.dirtyForms.add(id);
  }

  node.addEventListener("input", markDirty);

  return {
    destroy() {
      node.removeEventListener("input", markDirty);
      profile.dirtyForms.delete(id);
    },
  };
};
