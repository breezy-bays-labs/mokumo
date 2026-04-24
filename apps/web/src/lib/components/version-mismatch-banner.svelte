<script lang="ts">
  import { versionCheck } from "$lib/stores/version-check.svelte";

  // Non-blocking: renders only on confirmed mismatch, never on `pending`
  // or `unreachable` (a brief network hiccup during boot must not show a
  // false-positive banner).
  let state = $derived(versionCheck.state);
</script>

{#if state.status === "mismatch"}
  <div
    data-testid="version-mismatch-banner"
    role="status"
    aria-live="polite"
    class="w-full bg-amber-100 text-amber-900 border-b border-amber-300 px-4 py-2 text-sm"
  >
    ⚠ Admin UI version ({state.uiVersion}) does not match server API version ({state.serverVersion}).
    Some features may not work. Re-run your installer or contact support.
  </div>
{/if}
