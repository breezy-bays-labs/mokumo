<script lang="ts">
  import { Button } from "$lib/components/ui/button";
  import { toast } from "$lib/components/toast";
  import Copy from "@lucide/svelte/icons/copy";

  interface Props {
    url: string;
    label: string;
    testId?: string;
  }

  let { url, label, testId }: Props = $props();

  async function copyUrl() {
    try {
      await navigator.clipboard.writeText(url);
      toast.success("URL copied to clipboard");
    } catch {
      if (!window.isSecureContext) {
        toast.error("Clipboard requires HTTPS — copy the URL manually");
      } else {
        toast.error("Failed to copy URL");
      }
    }
  }
</script>

<div class="flex items-center gap-2">
  <code class="rounded bg-muted px-2 py-1 text-sm font-mono">{url}</code>
  <Button
    variant="ghost"
    size="icon"
    aria-label={label}
    data-testid={testId}
    onclick={copyUrl}
  >
    <Copy class="size-4" />
  </Button>
</div>
