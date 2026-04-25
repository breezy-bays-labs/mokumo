<script lang="ts">
  import { Alert, AlertDescription } from "$lib/components/ui/alert/index.js";

  interface Props {
    nextRetryInSeconds?: number;
  }

  let { nextRetryInSeconds = 5 }: Props = $props();

  let displayMs = $state(0);
  let countdownText = $derived(`${(displayMs / 1000).toFixed(1)}s`);

  $effect(() => {
    const totalMs = nextRetryInSeconds * 1000;
    displayMs = totalMs;
    const interval = setInterval(() => {
      displayMs = displayMs <= 100 ? totalMs : displayMs - 100;
    }, 100);
    return () => clearInterval(interval);
  });
</script>

<Alert
  data-testid="self-healing-banner"
  role="status"
  aria-live="polite"
  class="border-amber-300 bg-amber-100 text-amber-900 [&_[data-slot=alert-description]]:text-amber-900/90"
>
  <AlertDescription>
    Connection lost. Retrying automatically — next attempt in
    <span data-testid="self-healing-banner-next-retry" aria-hidden="true">{countdownText}</span>.
  </AlertDescription>
</Alert>
