<script lang="ts">
  let { children } = $props();
</script>

<div class="flex min-h-screen items-center justify-center bg-background p-4">
  <div class="w-full max-w-md space-y-6">
    <div class="flex flex-col items-center gap-2 text-center">
      <img
        src="/mokumo-cloud.png"
        alt="Mokumo"
        class="h-16 dark:invert pointer-events-none select-none"
        draggable="false"
        oncontextmenu={(e) => e.preventDefault()}
      />
      <span class="text-lg font-semibold tracking-tight">Mokumo Print</span>
    </div>
    {@render children()}
  </div>
</div>
