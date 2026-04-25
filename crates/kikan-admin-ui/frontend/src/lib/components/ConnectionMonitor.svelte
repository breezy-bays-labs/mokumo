<script lang="ts">
  import SelfHealingBanner from "./SelfHealingBanner.svelte";

  interface Props {
    pollIntervalMs?: number;
    nextRetryInSeconds?: number;
    initiallyOffline?: boolean;
    firstProbeDelayMs?: number;
  }

  let {
    pollIntervalMs = 400,
    nextRetryInSeconds = 5,
    initiallyOffline = false,
    firstProbeDelayMs = 0,
  }: Props = $props();

  // Tri-state: undefined until the first probe completes; then true/false.
  let probedOnline = $state<boolean | undefined>(undefined);

  // Show the banner if a probe has confirmed offline OR if we haven't probed
  // yet and the parent told us to start in the offline state.
  let showBanner = $derived(
    probedOnline === false || (probedOnline === undefined && initiallyOffline),
  );

  async function probe(signal: AbortSignal): Promise<void> {
    try {
      const response = await fetch("/api/platform/v1/branding", {
        cache: "no-store",
        signal,
      });
      // A reachable server returning 4xx/5xx is "broken backend", not "online" —
      // surface the reconnect banner the same way as a true network failure.
      if (!response.ok) throw new Error(`probe status ${response.status}`);
      if (!signal.aborted) probedOnline = true;
    } catch {
      if (!signal.aborted) probedOnline = false;
    }
  }

  $effect(() => {
    const controller = new AbortController();
    let timer: ReturnType<typeof setInterval> | undefined;

    const startup = setTimeout(() => {
      void probe(controller.signal);
      timer = setInterval(() => probe(controller.signal), pollIntervalMs);
    }, firstProbeDelayMs);

    return () => {
      controller.abort();
      clearTimeout(startup);
      if (timer) clearInterval(timer);
    };
  });
</script>

{#if showBanner}
  <SelfHealingBanner {nextRetryInSeconds} />
{/if}
