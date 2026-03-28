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

  function fallbackCopy(text: string): boolean {
    const textarea = document.createElement("textarea");
    textarea.value = text;
    textarea.style.position = "fixed";
    textarea.style.opacity = "0";
    document.body.appendChild(textarea);
    textarea.select();
    try {
      return document.execCommand("copy");
    } finally {
      document.body.removeChild(textarea);
    }
  }

  async function copyUrl() {
    try {
      await navigator.clipboard.writeText(url);
      toast.success("URL copied to clipboard");
    } catch {
      if (fallbackCopy(url)) {
        toast.success("URL copied to clipboard");
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
