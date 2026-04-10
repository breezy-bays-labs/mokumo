<script lang="ts">
  import "../app.css";
  import { ModeWatcher, mode } from "mode-watcher";
  import { Toaster, toastClasses, toast } from "$lib/components/toast";
  import { onMount } from "svelte";
  import { profile } from "$lib/stores/profile.svelte";
  import type { ServerStartupError } from "$lib/types/ServerStartupError";

  let { children } = $props();

  // Register beforeunload only when forms are dirty (avoids bfcache penalty).
  $effect(() => {
    if (profile.dirtyForms.size > 0) {
      const handler = (e: BeforeUnloadEvent) => {
        e.preventDefault();
        e.returnValue = "";
      };
      window.addEventListener("beforeunload", handler);
      return () => window.removeEventListener("beforeunload", handler);
    }
  });

  onMount(() => {
    if (!("__TAURI_INTERNALS__" in window)) return;

    let mounted = true;
    let unlistenClose: (() => void) | undefined;

    // Tauri: intercept native window close when forms have unsaved changes.
    import("@tauri-apps/api/window").then(({ getCurrentWindow }) => {
      if (!mounted) return;
      const win = getCurrentWindow();
      win
        .onCloseRequested(async (event) => {
          if (profile.dirtyForms.size === 0) return;
          event.preventDefault();
          const confirmed = window.confirm(
            "You have unsaved changes that will be lost. Leave anyway?",
          );
          if (confirmed) {
            profile.dirtyForms.clear();
            win.destroy();
          }
        })
        .then((fn) => {
          if (mounted) {
            unlistenClose = fn;
          } else {
            fn();
          }
        });
    });

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
      unlistenClose?.();
    };
  });

  function formatStartupError(err: ServerStartupError): string {
    switch (err.code) {
      case "migration_failed": {
        const backupNote = err.backup_path
          ? ` Your data is backed up at: ${err.backup_path}`
          : "";
        return `Migration failed (${err.path}): ${err.message}${backupNote}`;
      }
      case "schema_incompatible": {
        const backupNote = err.backup_path
          ? ` Backup at: ${err.backup_path}`
          : "";
        return `Database is newer than this version of Mokumo (${err.path}). Upgrade Mokumo or restore from backup.${backupNote}`;
      }
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
