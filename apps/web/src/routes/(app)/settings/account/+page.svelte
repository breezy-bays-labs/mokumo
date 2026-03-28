<script lang="ts">
  import { goto, invalidateAll } from "$app/navigation";
  import { page } from "$app/state";
  import ConfirmRegenDialog from "$lib/components/confirm-dialog/confirm-regen-dialog.svelte";
  import RecoveryCodes from "$lib/components/recovery-codes.svelte";
  import { Button } from "$lib/components/ui/button";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Label } from "$lib/components/ui/label";
  import KeyRound from "@lucide/svelte/icons/key-round";
  import type { PageData } from "./$types";

  let { data }: { data: PageData } = $props();

  let regeneratedCodes = $state<string[] | null>(null);
  let dialogOpen = $state(false);
  let savedChecked = $state(false);

  // Auto-open regen dialog when deep-linked with ?regen=true
  $effect(() => {
    if (page.url.searchParams.get("regen") === "true") {
      dialogOpen = true;
      goto("/settings/account", { replaceState: true });
    }
  });

  async function handleRegenerate(password: string) {
    const res = await fetch("/api/account/recovery-codes/regenerate", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ password }),
    });
    if (!res.ok) {
      const text = await res.text();
      let message = "Request failed";
      try {
        message = JSON.parse(text).message ?? message;
      } catch {
        message = text || message;
      }
      throw new Error(message);
    }
    const result = await res.json();
    regeneratedCodes = result.recovery_codes;
  }

  function handleDone() {
    regeneratedCodes = null;
    savedChecked = false;
    invalidateAll();
  }
</script>

<div class="space-y-6">
  <div>
    <h2 class="text-lg font-semibold">Recovery Codes</h2>
    <p class="text-sm text-muted-foreground">
      Recovery codes can be used to access your account if you lose your
      password.
    </p>
  </div>

  {#if regeneratedCodes}
    <div class="space-y-4">
      <div
        class="rounded-md bg-warning/10 border border-warning px-3 py-2 text-sm text-foreground"
      >
        Save these codes in a secure location. They will not be shown again.
      </div>
      <RecoveryCodes codes={regeneratedCodes} />
      <div class="flex items-center gap-2">
        <Checkbox id="saved-codes" bind:checked={savedChecked} />
        <Label for="saved-codes">I have saved my recovery codes</Label>
      </div>
      <Button disabled={!savedChecked} onclick={handleDone}>Done</Button>
    </div>
  {:else}
    <div class="flex items-center justify-between rounded-lg border p-4">
      <div class="flex items-center gap-3">
        <KeyRound class="h-5 w-5 text-muted-foreground" />
        <span class="text-sm">
          {data.recovery_codes_remaining} of 10 recovery codes remaining
        </span>
      </div>
      <Button variant="outline" onclick={() => (dialogOpen = true)}>
        Regenerate Recovery Codes
      </Button>
    </div>
  {/if}
</div>

<ConfirmRegenDialog
  bind:open={dialogOpen}
  title="Regenerate Recovery Codes"
  description="This will permanently invalidate your existing recovery codes. This cannot be undone."
  onConfirm={handleRegenerate}
/>
