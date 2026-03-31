import { profile } from "$lib/stores/profile.svelte";
import type { Action } from "svelte/action";

let nextId = 0;

/**
 * Svelte action that tracks whether a form has unsaved changes.
 *
 * Apply to any `<form>` element: `<form use:formDirty>`.
 * On `input`, the form is marked dirty. On `submit`, it is marked clean.
 * The action cleans up on destroy (e.g. when the form unmounts or navigates away).
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

  function markClean() {
    profile.dirtyForms.delete(id);
  }

  node.addEventListener("input", markDirty);
  node.addEventListener("submit", markClean);

  return {
    destroy() {
      node.removeEventListener("input", markDirty);
      node.removeEventListener("submit", markClean);
      profile.dirtyForms.delete(id);
    },
  };
};
