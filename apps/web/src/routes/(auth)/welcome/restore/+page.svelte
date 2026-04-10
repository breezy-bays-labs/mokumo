<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { goto } from "$app/navigation";
  import { apiFetch } from "$lib/api";
  import { Button } from "$lib/components/ui/button";
  import { Alert, AlertDescription } from "$lib/components/ui/alert";
  import Spinner from "$lib/components/spinner.svelte";
  import ArrowLeft from "@lucide/svelte/icons/arrow-left";
  import Upload from "@lucide/svelte/icons/upload";
  import CheckCircle from "@lucide/svelte/icons/check-circle";
  import type { RestoreValidateResponse } from "$lib/types/RestoreValidateResponse";
  import type { RestoreResponse } from "$lib/types/RestoreResponse";

  const RESTART_REDIRECT_MS = 2000;
  const RESTART_TIMEOUT_MS = 15000;

  type Source = { kind: "file"; file: File } | { kind: "path"; path: string };

  type RestoreState =
    | { kind: "picking" }
    | { kind: "validating"; fileName: string }
    | {
        kind: "valid";
        fileName: string;
        fileSize: number;
        schemaVersion: string | null;
      }
    | { kind: "invalid"; fileName: string; errorCode: string; message: string }
    | { kind: "importing" }
    | { kind: "import-failed"; message: string }
    | { kind: "restarting"; timedOut: boolean };

  let restoreState = $state<RestoreState>({ kind: "picking" });
  /** Stored between validate and import so the client can re-send the file. */
  let pendingSource = $state<Source | null>(null);
  /** Hidden file input for browser (non-Tauri) environments. */
  let fileInput: HTMLInputElement | undefined = $state();
  /** Timer handles stored so they can be cancelled in onDestroy. */
  let redirectTimer: ReturnType<typeof setTimeout> | undefined;
  let timeoutTimer: ReturnType<typeof setTimeout> | undefined;

  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  function errorCodeToMessage(code: string): string {
    switch (code) {
      case "not_mokumo_database":
        return "This file is not a valid Mokumo database. Please select a .db file created by Mokumo.";
      case "database_corrupt":
        return "This database file appears to be damaged. Try using a different backup file.";
      case "schema_incompatible":
        return "This database was created with a newer version of Mokumo. Please update Mokumo before importing.";
      case "production_db_exists":
        return "A shop database already exists.";
      case "restore_in_progress":
        return "Another import is already in progress.";
      case "rate_limited":
        return "Too many import attempts. Please wait before trying again.";
      default:
        return "An unexpected error occurred. Please try again.";
    }
  }

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  async function validate(source: Source): Promise<void> {
    const fileName =
      source.kind === "file"
        ? source.file.name
        : (source.path.split("/").pop() ??
          source.path.split("\\").pop() ??
          source.path);

    pendingSource = source;
    restoreState = { kind: "validating", fileName };

    let result: Awaited<ReturnType<typeof apiFetch<RestoreValidateResponse>>>;

    if (source.kind === "file") {
      const body = new FormData();
      body.append("file", source.file);
      result = await apiFetch<RestoreValidateResponse>(
        "/api/shop/restore/validate",
        {
          method: "POST",
          body,
        },
      );
    } else {
      result = await apiFetch<RestoreValidateResponse>(
        "/api/shop/restore/validate",
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ path: source.path }),
        },
      );
    }

    if (!result.ok) {
      const code = result.error.code;
      const message =
        code === "network_error" || code === "parse_error"
          ? result.error.message
          : errorCodeToMessage(code);
      restoreState = { kind: "invalid", fileName, errorCode: code, message };
      return;
    }

    if (!("data" in result)) return;
    const data = result.data;
    restoreState = {
      kind: "valid",
      fileName: data.file_name,
      fileSize: data.file_size,
      schemaVersion: data.schema_version,
    };
  }

  async function handleImportConfirmed(): Promise<void> {
    if (!pendingSource) return;

    restoreState = { kind: "importing" };

    let result: Awaited<ReturnType<typeof apiFetch<RestoreResponse>>>;

    if (pendingSource.kind === "file") {
      const body = new FormData();
      body.append("file", pendingSource.file);
      result = await apiFetch<RestoreResponse>("/api/shop/restore", {
        method: "POST",
        body,
      });
    } else {
      result = await apiFetch<RestoreResponse>("/api/shop/restore", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ path: pendingSource.path }),
      });
    }

    if (!result.ok) {
      const code = result.error.code;
      const message =
        code === "network_error" || code === "parse_error"
          ? result.error.message
          : errorCodeToMessage(code);
      restoreState = { kind: "import-failed", message };
      return;
    }

    restoreState = { kind: "restarting", timedOut: false };

    // Redirect after brief delay for the server to shut down and restart.
    redirectTimer = setTimeout(() => {
      window.location.href = "/login?restored=true";
    }, RESTART_REDIRECT_MS);

    // Show manual-restart message if the server takes too long.
    timeoutTimer = setTimeout(() => {
      if (restoreState.kind === "restarting") {
        restoreState = { kind: "restarting", timedOut: true };
      }
    }, RESTART_TIMEOUT_MS);
  }

  async function openPicker(): Promise<void> {
    if (isTauri) {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "Mokumo Database", extensions: ["db"] }],
      });
      if (typeof selected !== "string") {
        goto("/welcome");
        return;
      }
      await validate({ kind: "path", path: selected });
    } else {
      fileInput?.click();
    }
  }

  async function handleFileInputChange(e: Event): Promise<void> {
    const input = e.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    // Reset so the same file can be re-selected after an error.
    input.value = "";
    if (!file) {
      goto("/welcome");
      return;
    }
    await validate({ kind: "file", file });
  }

  function handleChooseDifferent(): void {
    pendingSource = null;
    openPicker();
  }

  onMount(() => {
    openPicker();
  });

  onDestroy(() => {
    clearTimeout(redirectTimer);
    clearTimeout(timeoutTimer);
  });
</script>

<!-- Hidden file input for browser environments -->
{#if !isTauri}
  <input
    bind:this={fileInput}
    type="file"
    accept=".db"
    class="hidden"
    onchange={handleFileInputChange}
  />
{/if}

<div class="flex flex-col gap-6">
  <div class="flex items-center gap-2">
    <Button
      variant="ghost"
      size="sm"
      onclick={() => goto("/welcome")}
      class="-ml-2"
      aria-label="Back to welcome"
    >
      <ArrowLeft class="h-4 w-4" />
    </Button>
    <h1 class="text-xl font-semibold">Open Existing Shop</h1>
  </div>

  {#if restoreState.kind === "picking"}
    <div
      class="flex flex-col items-center gap-3 text-center"
      data-testid="picking-state"
    >
      <Spinner size="lg" />
      <p class="text-sm text-muted-foreground">Opening file picker...</p>
    </div>
  {:else if restoreState.kind === "validating"}
    <div
      class="flex flex-col items-center gap-3 text-center"
      data-testid="validating-state"
    >
      <Spinner size="lg" />
      <p class="text-sm font-medium">{restoreState.fileName}</p>
      <p class="text-sm text-muted-foreground">Checking database...</p>
    </div>
  {:else if restoreState.kind === "valid"}
    <div class="flex flex-col gap-4" data-testid="valid-state">
      <div class="rounded-lg border p-4">
        <div class="flex items-start gap-3">
          <CheckCircle class="mt-0.5 h-5 w-5 shrink-0 text-green-600" />
          <div class="flex flex-col gap-1">
            <p class="text-sm font-medium">{restoreState.fileName}</p>
            <p class="text-xs text-muted-foreground">
              {formatFileSize(restoreState.fileSize)}
              {#if restoreState.schemaVersion}
                &middot; schema v{restoreState.schemaVersion}
              {/if}
            </p>
            <p class="text-xs text-green-700">Valid Mokumo database</p>
          </div>
        </div>
      </div>

      <Alert>
        <AlertDescription>
          You will need your existing login credentials after the import. Make
          sure you remember your email and password for this database.
        </AlertDescription>
      </Alert>

      <div class="flex flex-col gap-2">
        <Button onclick={handleImportConfirmed} data-testid="import-button">
          <Upload class="mr-2 h-4 w-4" />
          Import and Restart
        </Button>
        <Button
          variant="outline"
          onclick={handleChooseDifferent}
          data-testid="choose-different-button"
        >
          Choose Different File
        </Button>
      </div>
    </div>
  {:else if restoreState.kind === "invalid"}
    <div class="flex flex-col gap-4" data-testid="invalid-state">
      <Alert variant="destructive">
        <AlertDescription>{restoreState.message}</AlertDescription>
      </Alert>

      <div class="flex flex-col gap-2">
        <Button
          variant="outline"
          onclick={handleChooseDifferent}
          data-testid="choose-different-button"
        >
          Choose Different File
        </Button>
        <Button
          variant="ghost"
          onclick={() => goto("/welcome")}
          data-testid="back-button"
        >
          Back to Welcome
        </Button>
      </div>
    </div>
  {:else if restoreState.kind === "importing"}
    <div
      class="flex flex-col items-center gap-3 text-center"
      data-testid="importing-state"
    >
      <Spinner size="lg" />
      <p class="text-sm font-medium">Importing your shop data...</p>
      <p class="text-xs text-muted-foreground">This may take a moment.</p>
    </div>
  {:else if restoreState.kind === "import-failed"}
    <div class="flex flex-col gap-4" data-testid="import-failed-state">
      <Alert variant="destructive">
        <AlertDescription>{restoreState.message}</AlertDescription>
      </Alert>

      <div class="flex flex-col gap-2">
        <Button onclick={handleChooseDifferent} data-testid="try-again-button">
          Try Again
        </Button>
        <Button
          variant="ghost"
          onclick={() => goto("/welcome")}
          data-testid="back-button"
        >
          Back to Welcome
        </Button>
      </div>
    </div>
  {:else if restoreState.kind === "restarting"}
    <div
      class="flex flex-col items-center gap-4 text-center"
      data-testid="restarting-state"
    >
      {#if restoreState.timedOut}
        <p class="text-sm font-medium">Server did not restart in time.</p>
        <p class="text-xs text-muted-foreground">
          Please restart Mokumo manually, then sign in.
        </p>
        <Button
          variant="outline"
          size="sm"
          onclick={() => (window.location.href = "/login?restored=true")}
          data-testid="go-to-signin-button"
        >
          Go to Sign In
        </Button>
      {:else}
        <Spinner size="lg" />
        <p class="text-sm font-medium">Restarting server...</p>
        <p class="text-xs text-muted-foreground">
          You will be redirected to sign in shortly.
        </p>
      {/if}
    </div>
  {/if}
</div>
