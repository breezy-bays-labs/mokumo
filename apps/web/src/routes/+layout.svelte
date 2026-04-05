<script lang="ts">
  import "../app.css";
  import { ModeWatcher, mode } from "mode-watcher";
  import { Toaster, toastClasses, toast } from "$lib/components/toast";
  import { onMount } from "svelte";
  import type { ServerStartupError } from "$lib/types/ServerStartupError";

  let { children } = $props();

  onMount(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    let mounted = true;

    import("@tauri-apps/api/event").then(({ listen }) => {
      listen<ServerStartupError>("server-error", ({ payload }) => {
        toast.error("Server failed to restart", {
          description: formatStartupError(payload),
          duration: Infinity,
        });
      }).then((fn) => {
        if (mounted) {
          unlisten = fn;
        } else {
          fn();
        }
      });
    });

    let unlisten: (() => void) | undefined;

    return () => {
      mounted = false;
      unlisten?.();
    };
  });

  function formatStartupError(err: ServerStartupError): string {
    switch (err.code) {
      case "migration_failed":
        return `Migration failed (${err.path}): ${err.message}`;
      case "schema_incompatible":
        return `Database is newer than this version of Mokumo (${err.path}). Upgrade Mokumo or restore from backup.`;
      case "not_mokumo_database":
        return `File at ${err.path} is not a Mokumo database. Check your data directory.`;
      default: {
        const _exhaustive: never = err;
        return `Unexpected server error. Check the logs for details.`;
      }
    }
  }
</script>

<ModeWatcher defaultMode="system" />
<Toaster
  closeButton
  theme={mode.current}
  toastOptions={{ unstyled: true, classes: toastClasses }}
/>
{@render children()}
