<script lang="ts">
  import QRCode from "qrcode";

  let {
    value,
    size = 200,
  }: { value: string | null | undefined; size?: number } = $props();

  let canvasEl = $state<HTMLCanvasElement | undefined>(undefined);
  let renderError = $state(false);

  $effect(() => {
    if (!canvasEl || !value) {
      renderError = false;
      return;
    }

    // Read reactive dependencies synchronously before the async call —
    // values read inside a Promise callback are invisible to Svelte's tracker.
    const url = value;
    const width = size;

    renderError = false;

    let stale = false;
    QRCode.toCanvas(canvasEl, url, { width, margin: 1 }).catch(
      (err: unknown) => {
        if (stale) return;
        console.error("[qr-code] toCanvas failed:", err);
        renderError = true;
      },
    );

    return () => {
      stale = true;
    };
  });
</script>

<!--
  Canvas stays in the DOM at all times. Toggling it with {#if} would destroy
  the element and break bind:this, preventing re-render when value arrives.
-->
<canvas
  bind:this={canvasEl}
  class={!value || renderError ? "hidden" : ""}
  data-testid="qr-code"
  data-qr-value={value ?? ""}
  width={size}
  height={size}
></canvas>

{#if renderError}
  <div
    class="flex items-center justify-center rounded bg-muted"
    style="width:{size}px;height:{size}px"
    data-testid="qr-code-error"
  >
    <p class="px-2 text-center text-xs text-muted-foreground">
      QR code unavailable
    </p>
  </div>
{:else if !value}
  <div
    class="animate-pulse rounded bg-muted"
    style="width:{size}px;height:{size}px"
    data-testid="qr-code-placeholder"
  ></div>
{/if}
