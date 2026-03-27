<script lang="ts">
  import CloudSun from "@lucide/svelte/icons/cloud-sun";

  let { children } = $props();
</script>

<div class="flex min-h-screen items-center justify-center bg-background p-4">
  <div class="w-full max-w-md space-y-6">
    <div class="flex flex-col items-center gap-2 text-center">
      <div
        class="flex h-10 w-10 items-center justify-center rounded-lg bg-primary text-primary-foreground"
      >
        <CloudSun class="h-6 w-6" />
      </div>
      <span class="text-lg font-semibold tracking-tight">Mokumo Print</span>
    </div>
    {@render children()}
  </div>
</div>
