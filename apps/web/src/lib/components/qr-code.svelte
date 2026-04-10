<script lang="ts">
  import { onMount } from "svelte";
  import QRCode from "qrcode";

  let { value, size = 200 }: { value: string; size?: number } = $props();

  // oxlint-disable-next-line no-unassigned-vars -- assigned via bind:this
  let canvasEl: HTMLCanvasElement;

  onMount(() => {
    QRCode.toCanvas(canvasEl, value, { width: size, margin: 1 });
  });
</script>

<canvas
  bind:this={canvasEl}
  data-testid="qr-code"
  data-qr-value={value}
  width={size}
  height={size}
></canvas>
