<script lang="ts">
  import { cn } from "$lib/utils.js";

  type Status = "queued" | "building" | "ready" | "error" | "canceled";

  interface Props {
    status?: Status;
    label?: string;
    class?: string;
  }

  let { status = "ready", label, class: className }: Props = $props();

  const statusColors: Record<Status, string> = {
    queued: "bg-muted-foreground",
    building: "bg-warning",
    ready: "bg-success",
    error: "bg-error",
    canceled: "bg-muted-foreground/50",
  };

  const pulseStatuses: Set<Status> = new Set(["building"]);

  let dotClass = $derived(
    cn(
      "relative inline-block size-2 shrink-0 rounded-full",
      statusColors[status],
      className,
    ),
  );
</script>

<span
  class="inline-flex items-center gap-1.5"
  role="status"
  aria-label={label ? undefined : `Status: ${status}`}
>
  <span class={dotClass}>
    {#if pulseStatuses.has(status)}
      <span
        class="absolute inset-0 animate-ping rounded-full {statusColors[
          status
        ]} opacity-75"
      ></span>
    {/if}
  </span>
  {#if label}
    <span class="text-sm text-muted-foreground">{label}</span>
  {/if}
</span>
