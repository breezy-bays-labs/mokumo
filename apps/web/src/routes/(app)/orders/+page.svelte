<script lang="ts">
  import EmptyState from "$lib/components/empty-state.svelte";
  import ShoppingCart from "@lucide/svelte/icons/shopping-cart";
</script>

<EmptyState
  icon={ShoppingCart}
  title="Orders"
  subtitle="Track and manage production orders."
/>
