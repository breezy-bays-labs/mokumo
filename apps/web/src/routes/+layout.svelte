<script lang="ts">
  import "../app.css";
  import { ModeWatcher, mode } from "mode-watcher";
  import { Toaster, toastClasses } from "$lib/components/toast";

  let { children } = $props();
</script>

<ModeWatcher defaultMode="system" />
<Toaster
  closeButton
  theme={mode.current}
  toastOptions={{ unstyled: true, classes: toastClasses }}
/>
{@render children()}
