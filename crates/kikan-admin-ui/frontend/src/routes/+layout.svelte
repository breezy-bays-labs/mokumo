<script lang="ts">
  import "../app.css";
  import { ModeWatcher } from "mode-watcher";
  import { Toaster } from "$lib/components/ui/sonner/index.js";

  let { children } = $props();
</script>

<ModeWatcher />
<Toaster richColors closeButton position="top-right" />

{@render children()}
